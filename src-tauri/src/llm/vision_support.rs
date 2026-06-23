#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionSupport {
    Supported,
    Unsupported,
    Unknown,
}

pub fn gateway_vision_support(model_name: &str) -> VisionSupport {
    let lower = model_name.trim().to_lowercase();
    if lower.is_empty() {
        return VisionSupport::Unknown;
    }

    if matches!(
        lower.as_str(),
        "claude-sonnet-4-5" | "claude-ops" | "glm5.1"
    ) {
        return VisionSupport::Supported;
    }
    if lower.contains("claude") && (lower.contains("sonnet") || lower.contains("opus")) {
        return VisionSupport::Supported;
    }
    if lower.starts_with("deepseek") || lower.starts_with("qwen") {
        return VisionSupport::Unsupported;
    }

    VisionSupport::Unknown
}

pub fn supports_gateway_vision(model_name: &str) -> bool {
    gateway_vision_support(model_name) == VisionSupport::Supported
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_verified_anthropic_vision_models() {
        assert_eq!(
            gateway_vision_support("claude-sonnet-4-5"),
            VisionSupport::Supported
        );
        assert_eq!(
            gateway_vision_support("CLAUDE-OPS"),
            VisionSupport::Supported
        );
        assert_eq!(gateway_vision_support("glm5.1"), VisionSupport::Supported);
    }

    #[test]
    fn rejects_models_without_usable_anthropic_vision() {
        assert_eq!(
            gateway_vision_support("deepseek-v4-pro[1m]"),
            VisionSupport::Unsupported
        );
        assert_eq!(
            gateway_vision_support("qwen-plus"),
            VisionSupport::Unsupported
        );
    }

    #[test]
    fn unknown_models_do_not_send_images_by_default() {
        assert_eq!(
            gateway_vision_support("some-new-model"),
            VisionSupport::Unknown
        );
        assert!(!supports_gateway_vision("some-new-model"));
    }
}
