//! Product-level image task runtime tool.
//!
//! The LLM sees AIjia PI fields (`action`, `instruction`, `input_images`,
//! `output`) only. Provider-specific fields are produced by the gateway.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::AuthManager;
use crate::plugin::tool_trait::FileMeta;
use crate::runtime::path_auth::PathOp;
use crate::runtime::store::AuthorizedWorkspaceRef;
use crate::runtime::tools::builtin::workspace::{
    check_path_permission, resolve_and_authorize_path,
};
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::PermissionDecision;
use crate::runtime::tools::RuntimeTool;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::types::FileStorageRoot;
use crate::storage::file_store::AppStorage;

const TOOL_NAME: &str = "ImageTask";
const IMAGE_TASK_PATH: &str = "/aijia/v2/pi/image/tasks";
const DEFAULT_IMAGE_LOGICAL_MODEL: &str = "default-image";
const MAX_INPUT_IMAGE_BYTES: u64 = 15 * 1024 * 1024;

#[derive(Clone)]
pub struct ImageTaskDeps {
    pub auth_manager: Option<Arc<AuthManager>>,
    pub storage: Arc<AppStorage>,
    pub file_manager: Arc<FileManager>,
    pub workspace_path: std::path::PathBuf,
    pub authorized_workspace: Option<AuthorizedWorkspaceRef>,
    pub conversation_id: String,
    pub run_id: Option<String>,
    pub gateway_base_url: Option<String>,
}

pub struct ImageTaskRuntimeTool {
    deps: Arc<ImageTaskDeps>,
    client: Client,
}

impl ImageTaskRuntimeTool {
    pub fn new(deps: ImageTaskDeps) -> Self {
        Self {
            deps: Arc::new(deps),
            client: Client::new(),
        }
    }

    fn gateway_url(&self) -> String {
        let base = self
            .deps
            .gateway_base_url
            .clone()
            .unwrap_or_else(crate::environment::tenant_host);
        format!("{}{}", base.trim_end_matches('/'), IMAGE_TASK_PATH)
    }

    async fn load_input_images(
        &self,
        input: &ImageTaskToolInput,
        ctx: &ToolExecutionContext,
    ) -> Result<Vec<PiImageInput>, ToolError> {
        let mut images = Vec::with_capacity(input.input_images.len());

        for (idx, image) in input.input_images.iter().enumerate() {
            let resolved = self.resolve_input_image_path(image, ctx).await?;
            let meta = std::fs::metadata(&resolved).map_err(|err| {
                ToolError::ExecutionFailed(format!(
                    "failed to stat input image {}: {err}",
                    resolved.display()
                ))
            })?;
            if !meta.is_file() {
                return Err(validation_error(format!(
                    "input_images[{idx}] is not a file: {}",
                    resolved.display()
                )));
            }
            if meta.len() > MAX_INPUT_IMAGE_BYTES {
                return Err(validation_error(format!(
                    "input_images[{idx}] exceeds {} bytes",
                    MAX_INPUT_IMAGE_BYTES
                )));
            }

            let bytes = std::fs::read(&resolved).map_err(|err| {
                ToolError::ExecutionFailed(format!(
                    "failed to read input image {}: {err}",
                    resolved.display()
                ))
            })?;
            let mime_type = image
                .mime_type
                .clone()
                .or_else(|| detect_image_mime(&resolved));
            let mime_type = match mime_type {
                Some(mime) if is_supported_image_mime(&mime) => mime,
                Some(mime) => {
                    return Err(validation_error(format!(
                        "input_images[{idx}] unsupported mime_type: {mime}"
                    )));
                }
                None => {
                    return Err(validation_error(format!(
                        "input_images[{idx}] mime_type is required or must be inferable from extension"
                    )));
                }
            };

            images.push(PiImageInput {
                id: image
                    .file_id
                    .clone()
                    .unwrap_or_else(|| format!("input-{}", idx + 1)),
                kind: "image".to_string(),
                role: image.role.clone().unwrap_or_else(|| "source".to_string()),
                mime_type: Some(mime_type),
                data: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
                url: None,
                asset_id: image.file_id.clone(),
                weight: image.weight,
            });
        }

        Ok(images)
    }

    async fn resolve_input_image_path(
        &self,
        image: &ToolInputImage,
        ctx: &ToolExecutionContext,
    ) -> Result<std::path::PathBuf, ToolError> {
        if let Some(path) = image.file_path.as_deref().filter(|s| !s.trim().is_empty()) {
            return resolve_and_authorize_path(ctx, path, PathOp::Read).await;
        }

        let file_id = image
            .file_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                validation_error("each input image requires file_path or file_id".to_string())
            })?;
        let record = self.resolve_file_id_record(file_id)?.ok_or_else(|| {
            validation_error(format!(
                "input image file_id not found in this conversation: {file_id}"
            ))
        })?;
        self.resolve_record_to_existing_file(&record)
    }

    fn resolve_file_id_stored_path(&self, file_id: &str) -> Result<Option<String>, ToolError> {
        Ok(self
            .resolve_file_id_record(file_id)?
            .as_ref()
            .and_then(stored_path_from_record))
    }

    fn resolve_file_id_record(&self, file_id: &str) -> Result<Option<Value>, ToolError> {
        let conversation_id = self.deps.conversation_id.as_str();
        let uploaded = self
            .deps
            .storage
            .get_uploaded_file_for_conversation(file_id, conversation_id)
            .map_err(|err| ToolError::ExecutionFailed(format!("file lookup failed: {err}")))?;
        if uploaded.is_some() {
            return Ok(uploaded);
        }

        self.deps
            .storage
            .get_generated_file_for_conversation(file_id, conversation_id)
            .map_err(|err| ToolError::ExecutionFailed(format!("file lookup failed: {err}")))
    }

    fn conversation_dir(&self) -> std::path::PathBuf {
        self.deps
            .storage
            .base_dir()
            .join("conversations")
            .join(&self.deps.conversation_id)
    }

    fn output_storage_root(&self) -> (std::path::PathBuf, FileStorageRoot) {
        if let Some(workspace) = &self.deps.authorized_workspace {
            let kind = if workspace.id == "default" {
                "defaultFolder"
            } else {
                "authorizedWorkspace"
            };
            return (
                workspace.root_path.clone(),
                FileStorageRoot {
                    kind: kind.to_string(),
                    path: workspace.root_path.clone(),
                    display_name: Some(workspace.display_name.clone()),
                },
            );
        }
        (
            self.deps.workspace_path.clone(),
            FileStorageRoot {
                kind: "workspacePath".to_string(),
                path: self.deps.workspace_path.clone(),
                display_name: None,
            },
        )
    }

    fn resolve_stored_path_to_existing_file(
        &self,
        stored_path: &str,
    ) -> Result<std::path::PathBuf, ToolError> {
        let conv_dir = self.conversation_dir();
        if let Ok(path) = FileManager::resolve_existing_file_under_root(&conv_dir, stored_path) {
            return Ok(path);
        }
        self.deps
            .file_manager
            .resolve_existing_file(stored_path)
            .map_err(|err| ToolError::ExecutionFailed(format!("stored file unavailable: {err}")))
    }

    fn resolve_record_to_existing_file(
        &self,
        record: &Value,
    ) -> Result<std::path::PathBuf, ToolError> {
        let stored_path = stored_path_from_record(record).ok_or_else(|| {
            ToolError::ExecutionFailed("file record missing storedPath".to_string())
        })?;
        let storage_scope = record
            .get("storageScope")
            .or_else(|| record.get("storage_scope"))
            .and_then(Value::as_str)
            .unwrap_or("conversation");
        if storage_scope == "workspace" {
            if let Some(root) = record_storage_root_path(record) {
                return FileManager::resolve_existing_file_under_root(&root, &stored_path).map_err(
                    |err| ToolError::ExecutionFailed(format!("stored file unavailable: {err}")),
                );
            }
        }
        self.resolve_stored_path_to_existing_file(&stored_path)
    }

    async fn post_image_task(
        &self,
        request: &PiImageTaskRequest,
    ) -> Result<PiImageTaskResponse, ToolError> {
        let auth_manager = self.deps.auth_manager.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed("ImageTask requires an authenticated AIjia session".into())
        })?;
        let session_key = auth_manager
            .get_session_key()
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("session key unavailable: {err}")))?;

        let url = self.gateway_url();
        let response = self
            .client
            .post(url)
            .bearer_auth(session_key)
            .json(request)
            .send()
            .await
            .map_err(|err| {
                ToolError::ExecutionFailed(format!("image task request failed: {err}"))
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::ExecutionFailed(format!(
                "AIjia image task failed ({status}): {body}"
            )));
        }

        response.json::<PiImageTaskResponse>().await.map_err(|err| {
            ToolError::ExecutionFailed(format!("invalid image task response: {err}"))
        })
    }

    async fn persist_output_assets(
        &self,
        input: &ImageTaskToolInput,
        response: &PiImageTaskResponse,
    ) -> Result<Vec<PersistedImageFile>, ToolError> {
        if response.outputs.is_empty() {
            return Err(ToolError::ExecutionFailed(
                "AIjia image task returned no outputs".into(),
            ));
        }

        let mut files = Vec::with_capacity(response.outputs.len());
        for (idx, asset) in response.outputs.iter().enumerate() {
            let (bytes, data_mime) = match asset.data.as_deref() {
                Some(data) if !data.trim().is_empty() => decode_image_data(data)?,
                _ => {
                    let url = asset.url.as_deref().ok_or_else(|| {
                        ToolError::ExecutionFailed(format!(
                            "image task output {} has neither data nor url",
                            idx + 1
                        ))
                    })?;
                    self.download_image(url).await?
                }
            };

            let mime_type = sniff_image_mime(&bytes)
                .or_else(|| asset.mime_type.clone())
                .or(data_mime)
                .or_else(|| input.output.format.as_deref().and_then(mime_from_format));
            let ext = choose_extension(
                mime_type.as_deref(),
                asset.file_name.as_deref(),
                input.output.format.as_deref(),
            );
            let file_name = generated_file_name(response.task_id.as_deref(), idx, &ext);
            let (output_root, storage_root) = self.output_storage_root();
            let output_subdir = format!("generated/{}/images", self.deps.conversation_id);
            let info = FileManager::write_file_under_root(
                &output_root,
                &output_subdir,
                &file_name,
                &bytes,
            )
            .map_err(|err| {
                ToolError::ExecutionFailed(format!("failed to write generated image: {err}"))
            })?;

            let file_id = uuid::Uuid::new_v4().to_string();
            let description = truncate_chars(
                &format!("ImageTask {}: {}", input.action, input.instruction.trim()),
                300,
            );
            self.deps
                .storage
                .insert_generated_file_with_storage(
                    &file_id,
                    &self.deps.conversation_id,
                    None,
                    &info.file_name,
                    &info.stored_path,
                    &info.file_type,
                    info.file_size as i64,
                    "image",
                    Some(&description),
                    1,
                    true,
                    None,
                    None,
                    None,
                    "workspace",
                    Some(storage_root),
                )
                .map_err(|err| {
                    ToolError::ExecutionFailed(format!(
                        "failed to register generated image file: {err}"
                    ))
                })?;

            let file_meta = FileMeta {
                file_id: file_id.clone(),
                file_name: info.file_name.clone(),
                requested_format: input
                    .output
                    .format
                    .clone()
                    .unwrap_or_else(|| ext_to_requested_format(&ext).to_string()),
                actual_format: info.file_type.clone(),
                file_size: info.file_size,
                stored_path: info.stored_path.clone(),
                category: "image".to_string(),
            };

            files.push(PersistedImageFile {
                file_id,
                file_name: info.file_name,
                stored_path: info.stored_path,
                file_type: info.file_type,
                file_size: info.file_size,
                mime_type,
                source_asset_id: asset.id.clone(),
                file_meta,
            });
        }

        Ok(files)
    }

    async fn download_image(&self, url: &str) -> Result<(Vec<u8>, Option<String>), ToolError> {
        let response =
            self.client.get(url).send().await.map_err(|err| {
                ToolError::ExecutionFailed(format!("image download failed: {err}"))
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(ToolError::ExecutionFailed(format!(
                "image download failed ({status})"
            )));
        }
        let mime_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let bytes = response
            .bytes()
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("image download failed: {err}")))?;
        Ok((bytes.to_vec(), mime_type))
    }
}

#[async_trait]
impl RuntimeTool for ImageTaskRuntimeTool {
    fn id(&self) -> &str {
        TOOL_NAME
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        TOOL_CATALOG
            .get(TOOL_NAME)
            .unwrap_or_else(|| ToolDefinition::new(TOOL_NAME, "Create or edit images"))
    }

    fn default_destructive(&self) -> bool {
        true
    }

    async fn check_permissions(
        &self,
        input: &Value,
        ctx: &ToolExecutionContext,
    ) -> Option<PermissionDecision> {
        for path in input_image_paths_for_permission(input, self) {
            let path_input = json!({ "file_path": path });
            if let Some(decision) = check_path_permission(&path_input, ctx, PathOp::Read, TOOL_NAME)
            {
                return Some(decision);
            }
        }
        None
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let input: ImageTaskToolInput = serde_json::from_value(input)
            .map_err(|err| validation_error(format!("invalid input: {err}")))?;
        validate_tool_input(&input)?;

        let pi_inputs = self.load_input_images(&input, &ctx).await?;
        let request = build_pi_request(
            &input,
            pi_inputs,
            ImageTaskRequestMeta {
                conversation_id: self.deps.conversation_id.clone(),
                run_id: self
                    .deps
                    .run_id
                    .clone()
                    .or_else(|| Some(ctx.run_id.as_str().to_string())),
            },
        );

        let response = self.post_image_task(&request).await?;
        let files = self.persist_output_assets(&input, &response).await?;
        let content = build_result_content(&response, &files);
        let data = build_result_data(&response, &files);
        let mut result = ToolResult::new(TOOL_NAME, content, Some(data));
        result.file_meta = files.first().map(|file| file.file_meta.clone());
        Ok(result)
    }
}

#[derive(Debug, Deserialize)]
struct ImageTaskToolInput {
    action: String,
    instruction: String,
    #[serde(default, alias = "inputImages")]
    input_images: Vec<ToolInputImage>,
    #[serde(default)]
    output: ImageTaskOutputInput,
}

#[derive(Debug, Deserialize)]
struct ToolInputImage {
    #[serde(default, alias = "filePath", alias = "path")]
    file_path: Option<String>,
    #[serde(default, alias = "fileId")]
    file_id: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default, alias = "mimeType")]
    mime_type: Option<String>,
    #[serde(default)]
    weight: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
struct ImageTaskOutputInput {
    #[serde(default)]
    count: Option<u32>,
    #[serde(default, alias = "aspectRatio")]
    aspect_ratio: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    quality: Option<String>,
}

#[derive(Debug, Serialize)]
struct PiImageTaskRequest {
    schema_version: String,
    conversation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
    action: String,
    instruction: PiInstruction,
    inputs: Vec<PiImageInput>,
    output: PiOutput,
    model_policy: PiModelPolicy,
    options: Value,
    client: PiClientInfo,
}

#[derive(Debug, Serialize)]
struct PiInstruction {
    text: String,
}

#[derive(Debug, Serialize)]
struct PiImageInput {
    id: String,
    kind: String,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    asset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    weight: Option<f64>,
}

#[derive(Debug, Serialize)]
struct PiOutput {
    count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    aspect_ratio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quality: Option<String>,
}

#[derive(Debug, Serialize)]
struct PiModelPolicy {
    mode: String,
    logical_model: String,
    allowed_capabilities: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PiClientInfo {
    name: String,
    version: String,
    platform: String,
}

#[derive(Debug, Deserialize)]
struct PiImageTaskResponse {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default, alias = "assets")]
    outputs: Vec<PiOutputAsset>,
    #[serde(default)]
    usage: Option<Value>,
    #[serde(default)]
    cost: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct PiOutputAsset {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    file_name: Option<String>,
}

struct ImageTaskRequestMeta {
    conversation_id: String,
    run_id: Option<String>,
}

#[derive(Clone)]
struct PersistedImageFile {
    file_id: String,
    file_name: String,
    stored_path: String,
    file_type: String,
    file_size: u64,
    mime_type: Option<String>,
    source_asset_id: Option<String>,
    file_meta: FileMeta,
}

fn validate_tool_input(input: &ImageTaskToolInput) -> Result<(), ToolError> {
    match input.action.as_str() {
        "image.create" | "image.edit" | "image.variation" => {}
        other => {
            return Err(validation_error(format!(
                "unsupported action: {other}; expected image.create, image.edit, or image.variation"
            )));
        }
    }
    if input.instruction.trim().is_empty() {
        return Err(validation_error("instruction is required".to_string()));
    }
    if input.action != "image.create" && input.input_images.is_empty() {
        return Err(validation_error(format!(
            "{} requires at least one input image",
            input.action
        )));
    }
    if let Some(count) = input.output.count {
        if !(1..=10).contains(&count) {
            return Err(validation_error("output.count must be 1..10".to_string()));
        }
    }
    if let Some(format) = input.output.format.as_deref() {
        if !matches!(format, "png" | "jpeg" | "jpg" | "webp") {
            return Err(validation_error(format!(
                "output.format must be png, jpeg, jpg, or webp; got {format}"
            )));
        }
    }
    if let Some(quality) = input.output.quality.as_deref() {
        if !matches!(quality, "standard" | "high") {
            return Err(validation_error(format!(
                "output.quality must be standard or high; got {quality}"
            )));
        }
    }
    for (idx, image) in input.input_images.iter().enumerate() {
        if image.file_path.as_deref().unwrap_or("").trim().is_empty()
            && image.file_id.as_deref().unwrap_or("").trim().is_empty()
        {
            return Err(validation_error(format!(
                "input_images[{idx}] requires file_path or file_id"
            )));
        }
        if let Some(role) = image.role.as_deref() {
            if !matches!(
                role,
                "source" | "reference" | "style_reference" | "composition_reference" | "mask"
            ) {
                return Err(validation_error(format!(
                    "input_images[{idx}].role is unsupported: {role}"
                )));
            }
        }
    }
    Ok(())
}

fn build_pi_request(
    input: &ImageTaskToolInput,
    images: Vec<PiImageInput>,
    meta: ImageTaskRequestMeta,
) -> PiImageTaskRequest {
    let (output_width, output_height) = normalized_output_dimensions(&input.output);
    PiImageTaskRequest {
        schema_version: "aijia.pi.image_task.v1".to_string(),
        conversation_id: meta.conversation_id,
        run_id: meta.run_id,
        action: input.action.clone(),
        instruction: PiInstruction {
            text: input.instruction.trim().to_string(),
        },
        inputs: images,
        output: PiOutput {
            count: input.output.count.unwrap_or(1),
            aspect_ratio: input.output.aspect_ratio.clone(),
            width: output_width,
            height: output_height,
            format: input
                .output
                .format
                .clone()
                .or_else(|| Some("png".to_string())),
            quality: input.output.quality.clone(),
        },
        model_policy: PiModelPolicy {
            mode: "auto".to_string(),
            logical_model: DEFAULT_IMAGE_LOGICAL_MODEL.to_string(),
            allowed_capabilities: vec!["image_generation".to_string()],
        },
        options: json!({}),
        client: PiClientInfo {
            name: "aijia-desktop".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            platform: client_platform(),
        },
    }
}

fn normalized_output_dimensions(output: &ImageTaskOutputInput) -> (Option<u32>, Option<u32>) {
    if output.width.is_some() || output.height.is_some() {
        return (output.width, output.height);
    }

    match output.aspect_ratio.as_deref() {
        Some("16:9") => (Some(2816), Some(1584)),
        Some("9:16") => (Some(1584), Some(2816)),
        Some("4:3") => (Some(2304), Some(1728)),
        Some("3:4") => (Some(1728), Some(2304)),
        _ => (Some(2048), Some(2048)),
    }
}

fn build_result_content(response: &PiImageTaskResponse, files: &[PersistedImageFile]) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "ImageTask completed. taskId={} status={}",
        response.task_id.as_deref().unwrap_or(""),
        response.status.as_deref().unwrap_or("completed")
    ));
    for file in files {
        lines.push(format!(
            "fileId: {} name: {} storedPath: {}",
            file.file_id, file.file_name, file.stored_path
        ));
    }
    lines.join("\n")
}

fn build_result_data(response: &PiImageTaskResponse, files: &[PersistedImageFile]) -> Value {
    let files: Vec<Value> = files
        .iter()
        .map(|file| {
            json!({
                "fileId": file.file_id,
                "fileName": file.file_name,
                "storedPath": file.stored_path,
                "fileType": file.file_type,
                "fileSize": file.file_size,
                "mimeType": file.mime_type,
                "sourceAssetId": file.source_asset_id,
            })
        })
        .collect();
    json!({
        "ok": true,
        "taskId": response.task_id,
        "status": response.status.as_deref().unwrap_or("completed"),
        "files": files,
        "usage": response.usage,
        "cost": response.cost,
    })
}

fn input_image_paths_for_permission(input: &Value, tool: &ImageTaskRuntimeTool) -> Vec<String> {
    let Some(images) = input
        .get("input_images")
        .or_else(|| input.get("inputImages"))
    else {
        return Vec::new();
    };
    let Some(images) = images.as_array() else {
        return Vec::new();
    };
    images
        .iter()
        .filter_map(|image| {
            image
                .get("file_path")
                .or_else(|| image.get("filePath"))
                .or_else(|| image.get("path"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    image
                        .get("file_id")
                        .or_else(|| image.get("fileId"))
                        .and_then(Value::as_str)
                        .and_then(|file_id| {
                            tool.resolve_file_id_stored_path(file_id).ok().flatten()
                        })
                })
        })
        .collect()
}

fn stored_path_from_record(record: &Value) -> Option<String> {
    record
        .get("storedPath")
        .or_else(|| record.get("stored_path"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn record_storage_root_path(record: &Value) -> Option<std::path::PathBuf> {
    record
        .get("storageRoot")
        .or_else(|| record.get("storage_root"))
        .and_then(|value| value.get("path"))
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .map(std::path::PathBuf::from)
}

fn decode_image_data(data: &str) -> Result<(Vec<u8>, Option<String>), ToolError> {
    let trimmed = data.trim();
    let (payload, mime_type) = if let Some(rest) = trimmed.strip_prefix("data:") {
        let comma = rest.find(',').ok_or_else(|| {
            ToolError::ExecutionFailed("invalid data URL returned by image task".into())
        })?;
        let meta = &rest[..comma];
        let mime = meta
            .split(';')
            .next()
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        (&rest[comma + 1..], mime)
    } else {
        (trimmed, None)
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.as_bytes())
        .map_err(|err| ToolError::ExecutionFailed(format!("invalid image base64: {err}")))?;
    Ok((bytes, mime_type))
}

fn generated_file_name(task_id: Option<&str>, idx: usize, ext: &str) -> String {
    let prefix = task_id
        .map(sanitize_file_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    format!("image-task-{}-{}.{}", prefix, idx + 1, ext)
}

fn sanitize_file_component(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

fn detect_image_mime(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png".to_string()),
        "jpg" | "jpeg" => Some("image/jpeg".to_string()),
        "webp" => Some("image/webp".to_string()),
        "gif" => Some("image/gif".to_string()),
        _ => None,
    }
}

fn sniff_image_mime(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("image/png".to_string());
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg".to_string());
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp".to_string());
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif".to_string());
    }
    None
}

fn is_supported_image_mime(mime: &str) -> bool {
    matches!(
        mime,
        "image/png" | "image/jpeg" | "image/jpg" | "image/webp" | "image/gif"
    )
}

fn choose_extension(
    mime: Option<&str>,
    file_name: Option<&str>,
    requested: Option<&str>,
) -> String {
    if let Some(ext) = file_name
        .and_then(|name| Path::new(name).extension())
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .filter(|ext| matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif"))
    {
        return if ext == "jpeg" {
            "jpg".to_string()
        } else {
            ext
        };
    }
    if let Some(ext) = mime.and_then(ext_from_mime) {
        return ext.to_string();
    }
    if let Some(ext) = requested.and_then(ext_from_format) {
        return ext.to_string();
    }
    "png".to_string()
}

fn ext_from_mime(mime: &str) -> Option<&'static str> {
    match mime {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

fn mime_from_format(format: &str) -> Option<String> {
    match format {
        "png" => Some("image/png".to_string()),
        "jpeg" | "jpg" => Some("image/jpeg".to_string()),
        "webp" => Some("image/webp".to_string()),
        _ => None,
    }
}

fn ext_from_format(format: &str) -> Option<&'static str> {
    match format {
        "png" => Some("png"),
        "jpeg" | "jpg" => Some("jpg"),
        "webp" => Some("webp"),
        _ => None,
    }
}

fn ext_to_requested_format(ext: &str) -> &str {
    match ext {
        "jpg" => "jpeg",
        other => other,
    }
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

fn validation_error(message: String) -> ToolError {
    ToolError::InputValidationError {
        tool_name: TOOL_NAME.to_string(),
        message,
    }
}

fn client_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn build_pi_request_uses_product_schema_without_provider_fields() {
        let input = ImageTaskToolInput {
            action: "image.edit".to_string(),
            instruction: "make the product photo warmer".to_string(),
            input_images: Vec::new(),
            output: ImageTaskOutputInput {
                count: Some(1),
                aspect_ratio: Some("1:1".to_string()),
                width: None,
                height: None,
                format: Some("png".to_string()),
                quality: Some("high".to_string()),
            },
        };
        let pi_input = PiImageInput {
            id: "ref-1".to_string(),
            kind: "image".to_string(),
            role: "source".to_string(),
            mime_type: Some("image/png".to_string()),
            data: Some("AAAA".to_string()),
            url: None,
            asset_id: None,
            weight: None,
        };

        let request = build_pi_request(
            &input,
            vec![pi_input],
            ImageTaskRequestMeta {
                conversation_id: "conv-1".to_string(),
                run_id: Some("run-1".to_string()),
            },
        );
        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["schema_version"], "aijia.pi.image_task.v1");
        assert_eq!(value["action"], "image.edit");
        assert_eq!(
            value["instruction"]["text"],
            "make the product photo warmer"
        );
        assert_eq!(value["inputs"][0]["kind"], "image");
        assert_eq!(value["inputs"][0]["data"], "AAAA");
        assert_eq!(value["output"]["width"], 2048);
        assert_eq!(value["output"]["height"], 2048);
        assert_eq!(value["model_policy"]["logical_model"], "default-image");
        assert!(value.get("image").is_none());
        assert!(value.get("response_format").is_none());
    }

    #[test]
    fn default_output_dimensions_keep_aspect_ratio_above_provider_minimum() {
        let mut output = ImageTaskOutputInput {
            aspect_ratio: Some("3:4".to_string()),
            ..Default::default()
        };
        assert_eq!(
            normalized_output_dimensions(&output),
            (Some(1728), Some(2304))
        );

        output.aspect_ratio = Some("4:3".to_string());
        assert_eq!(
            normalized_output_dimensions(&output),
            (Some(2304), Some(1728))
        );

        output.aspect_ratio = None;
        assert_eq!(
            normalized_output_dimensions(&output),
            (Some(2048), Some(2048))
        );
    }

    #[tokio::test]
    async fn persist_base64_output_registers_generated_file() {
        let storage_dir = TempDir::new().unwrap();
        let workspace_dir = TempDir::new().unwrap();
        let storage = Arc::new(AppStorage::new(storage_dir.path()).unwrap());
        storage
            .create_conversation("conv-1", "Image test")
            .expect("create conversation");
        let file_manager = Arc::new(FileManager::new(workspace_dir.path()));
        let tool = ImageTaskRuntimeTool::new(ImageTaskDeps {
            auth_manager: None,
            storage: storage.clone(),
            file_manager,
            workspace_path: workspace_dir.path().to_path_buf(),
            authorized_workspace: Some(AuthorizedWorkspaceRef {
                id: "test-workspace".to_string(),
                root_path: workspace_dir.path().to_path_buf(),
                display_name: "Test workspace".to_string(),
            }),
            conversation_id: "conv-1".to_string(),
            run_id: Some("run-1".to_string()),
            gateway_base_url: None,
        });
        let input = ImageTaskToolInput {
            action: "image.create".to_string(),
            instruction: "draw a simple icon".to_string(),
            input_images: Vec::new(),
            output: ImageTaskOutputInput {
                format: Some("png".to_string()),
                ..Default::default()
            },
        };
        let response = PiImageTaskResponse {
            task_id: Some("task-1".to_string()),
            status: Some("completed".to_string()),
            outputs: vec![PiOutputAsset {
                id: Some("asset-1".to_string()),
                mime_type: Some("image/png".to_string()),
                data: Some(base64::engine::general_purpose::STANDARD.encode([1_u8, 2, 3])),
                url: None,
                file_name: None,
            }],
            usage: None,
            cost: None,
        };

        let files = tool.persist_output_assets(&input, &response).await.unwrap();

        assert_eq!(files.len(), 1);
        assert!(
            files[0].stored_path.starts_with("generated/conv-1/images/"),
            "ImageTask outputs should be conversation-namespaced inside the workspace"
        );
        let full_path = workspace_dir.path().join(&files[0].stored_path);
        assert_eq!(std::fs::read(full_path).unwrap(), vec![1_u8, 2, 3]);
        assert!(
            !storage_dir
                .path()
                .join("conversations")
                .join("conv-1")
                .join(&files[0].stored_path)
                .exists(),
            "new ImageTask outputs should not be written to the conversation directory"
        );
        let record = storage
            .get_generated_file_for_conversation(&files[0].file_id, "conv-1")
            .unwrap()
            .expect("registered generated file");
        assert_eq!(record["category"], "image");
        assert_eq!(record["fileType"], "png");
        assert_eq!(record["storageScope"], "workspace");
        let workspace_root = workspace_dir.path().to_string_lossy().to_string();
        assert_eq!(
            record["storageRoot"]["path"].as_str(),
            Some(workspace_root.as_str())
        );
    }

    #[tokio::test]
    async fn persist_output_prefers_detected_mime_over_requested_format() {
        let storage_dir = TempDir::new().unwrap();
        let workspace_dir = TempDir::new().unwrap();
        let storage = Arc::new(AppStorage::new(storage_dir.path()).unwrap());
        storage
            .create_conversation("conv-1", "Image test")
            .expect("create conversation");
        let file_manager = Arc::new(FileManager::new(workspace_dir.path()));
        let tool = ImageTaskRuntimeTool::new(ImageTaskDeps {
            auth_manager: None,
            storage: storage.clone(),
            file_manager,
            workspace_path: workspace_dir.path().to_path_buf(),
            authorized_workspace: Some(AuthorizedWorkspaceRef {
                id: "test-workspace".to_string(),
                root_path: workspace_dir.path().to_path_buf(),
                display_name: "Test workspace".to_string(),
            }),
            conversation_id: "conv-1".to_string(),
            run_id: Some("run-1".to_string()),
            gateway_base_url: None,
        });
        let input = ImageTaskToolInput {
            action: "image.create".to_string(),
            instruction: "draw a simple icon".to_string(),
            input_images: Vec::new(),
            output: ImageTaskOutputInput {
                format: Some("png".to_string()),
                ..Default::default()
            },
        };
        let jpeg_bytes = [
            0xff_u8, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F',
        ];
        let response = PiImageTaskResponse {
            task_id: Some("task-1".to_string()),
            status: Some("completed".to_string()),
            outputs: vec![PiOutputAsset {
                id: Some("asset-1".to_string()),
                mime_type: Some("image/png".to_string()),
                data: Some(base64::engine::general_purpose::STANDARD.encode(jpeg_bytes)),
                url: None,
                file_name: None,
            }],
            usage: None,
            cost: None,
        };

        let files = tool.persist_output_assets(&input, &response).await.unwrap();

        assert!(files[0].file_name.ends_with(".jpg"));
        assert_eq!(files[0].file_type, "jpeg");
        assert_eq!(files[0].mime_type.as_deref(), Some("image/jpeg"));
    }
}
