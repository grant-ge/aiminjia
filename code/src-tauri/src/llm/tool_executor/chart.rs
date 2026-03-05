//! generate_chart handler — Plotly interactive HTML charts.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::FileMeta;
use crate::python::runner::PythonRunner;

use super::FileGenResult;
use super::require_str;

/// 5. generate_chart — create a Plotly interactive HTML chart.
pub(crate) async fn handle_generate_chart(ctx: &PluginContext, args: &Value) -> Result<FileGenResult> {
    let chart_type = require_str(args, "chart_type")?;
    let title = require_str(args, "title")?;
    let data = args
        .get("data")
        .ok_or_else(|| anyhow!("Missing required argument: data"))?;
    let options = args.get("options").cloned().unwrap_or(json!({}));

    let chart_filename = format!(
        "chart_{}.html",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    );
    let chart_dir = ctx.workspace_path.join("charts");
    std::fs::create_dir_all(&chart_dir)?;
    let output_path = chart_dir.join(&chart_filename);

    // Write data and options to temp files (avoids triple-quote injection)
    let temp_dir = ctx.workspace_path.join("temp");
    std::fs::create_dir_all(&temp_dir)?;
    let data_temp = temp_dir.join(format!(
        "chart_data_{}.json",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    ));
    let options_temp = temp_dir.join(format!(
        "chart_opts_{}.json",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    ));
    std::fs::write(&data_temp, serde_json::to_string(data).unwrap_or_else(|_| "{}".into()))?;
    std::fs::write(&options_temp, serde_json::to_string(&options).unwrap_or_else(|_| "{}".into()))?;

    let python_code = build_chart_python(
        chart_type,
        title,
        &data_temp.to_string_lossy(),
        &options_temp.to_string_lossy(),
        &output_path.to_string_lossy(),
    );

    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());
    let result = runner.execute(&python_code).await?;

    // Clean up temp files if Python didn't
    let _ = std::fs::remove_file(&data_temp);
    let _ = std::fs::remove_file(&options_temp);

    if result.exit_code != 0 {
        return Err(anyhow!(
            "Chart generation failed (exit {}):\n{}",
            result.exit_code,
            if result.stderr.is_empty() {
                &result.stdout
            } else {
                &result.stderr
            }
        ));
    }

    // Write the file info (the Python script already saved the HTML).
    let stored_path = format!("charts/{}", chart_filename);
    let file_size = std::fs::metadata(&output_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let file_id = Uuid::new_v4().to_string();
    if let Err(e) = ctx.storage.insert_generated_file(
        &file_id,
        &ctx.conversation_id,
        None,
        &chart_filename,
        &stored_path,
        "html",
        file_size as i64,
        "chart",
        Some(title),
        1,
        true,
        None,
        None,
        None,
    ) {
        let _ = std::fs::remove_file(&output_path);
        return Err(e.into());
    }

    let content = serde_json::to_string_pretty(&json!({
        "fileId": file_id,
        "fileName": chart_filename,
        "storedPath": stored_path,
        "fileSize": file_size,
        "chartType": chart_type,
    }))?;

    let file_meta = FileMeta {
        file_id,
        file_name: chart_filename,
        requested_format: "html".to_string(),
        actual_format: "html".to_string(),
        file_size,
        stored_path,
        category: "chart".to_string(),
    };

    Ok(FileGenResult {
        content,
        file_meta,
        is_degraded: false,
        degradation_notice: None,
    })
}

fn build_chart_python(
    chart_type: &str,
    title: &str,
    data_file_path: &str,
    options_file_path: &str,
    output_path: &str,
) -> String {
    let escaped_chart_type = super::util::py_escape(chart_type);
    let escaped_title = super::util::py_escape(title);
    let escaped_output_path = super::util::py_escape(output_path);
    let escaped_data_path = super::util::py_escape(data_file_path);
    let escaped_options_path = super::util::py_escape(options_file_path);

    format!(
        r#"
import plotly.graph_objects as go
import json
import os

with open('{data_path}', 'r', encoding='utf-8') as _f:
    data = json.load(_f)
os.remove('{data_path}')
with open('{options_path}', 'r', encoding='utf-8') as _f:
    options = json.load(_f)
os.remove('{options_path}')

chart_type = '{chart_type}'
title = '{title}'
output_path = r'{output_path}'

labels = data.get('labels', [])
values = data.get('values', [])
fig = go.Figure()

if chart_type == 'bar':
    if isinstance(values[0], list) if values else False:
        series_names = data.get('series_names', [f'Series {{i+1}}' for i in range(len(values))])
        for i, v in enumerate(values):
            name = series_names[i] if i < len(series_names) else f'Series {{i+1}}'
            fig.add_trace(go.Bar(x=labels, y=v, name=name))
        fig.update_layout(barmode='group')
    else:
        fig.add_trace(go.Bar(x=labels, y=values))

elif chart_type == 'line':
    if isinstance(values[0], list) if values else False:
        series_names = data.get('series_names', [f'Series {{i+1}}' for i in range(len(values))])
        for i, v in enumerate(values):
            name = series_names[i] if i < len(series_names) else f'Series {{i+1}}'
            fig.add_trace(go.Scatter(x=labels, y=v, mode='lines+markers', name=name))
    else:
        fig.add_trace(go.Scatter(x=labels, y=values, mode='lines+markers'))

elif chart_type == 'scatter':
    x_vals = data.get('x', [])
    y_vals = data.get('y', [])
    fig.add_trace(go.Scatter(x=x_vals, y=y_vals, mode='markers',
                             marker=dict(opacity=0.7)))
    fig.update_xaxes(title_text=data.get('x_label', 'X'))
    fig.update_yaxes(title_text=data.get('y_label', 'Y'))

elif chart_type == 'box':
    box_data = data.get('groups', [values])
    for i, group in enumerate(box_data):
        name = labels[i] if i < len(labels) else f'Group {{i+1}}'
        fig.add_trace(go.Box(y=group, name=name))

elif chart_type == 'heatmap':
    matrix = data.get('matrix', [[]])
    y_labels = data.get('y_labels', [])
    fig.add_trace(go.Heatmap(
        z=matrix,
        x=labels if labels else None,
        y=y_labels if y_labels else None,
        colorscale='YlOrRd',
    ))

elif chart_type == 'pie':
    fig.add_trace(go.Pie(labels=labels, values=values))

elif chart_type == 'histogram':
    fig.add_trace(go.Histogram(x=values,
                                nbinsx=options.get('bins', 20)))

fig.update_layout(
    title=dict(text=title, x=0.5),
    template='plotly_white',
    font=dict(family='system-ui, -apple-system, sans-serif'),
    margin=dict(l=60, r=40, t=60, b=60),
    width=options.get('width', 900),
    height=options.get('height', 550),
)

# Save as self-contained HTML (plotly.js inlined, works offline)
fig.write_html(output_path, include_plotlyjs=True, full_html=True)
print(f"Chart saved to {{output_path}}")
"#,
        data_path = escaped_data_path,
        options_path = escaped_options_path,
        chart_type = escaped_chart_type,
        title = escaped_title,
        output_path = escaped_output_path,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_chart_python_bar() {
        let code = build_chart_python("bar", "My Chart", "/tmp/chart_data.json", "/tmp/chart_opts.json", "/tmp/chart.html");
        assert!(code.contains("plotly"));
        assert!(code.contains("chart_type = 'bar'"));
        assert!(code.contains("write_html"));
        assert!(code.contains("/tmp/chart.html"));
        // Verify temp-file protocol: reads data from file, not inline JSON
        assert!(code.contains("json.load("));
        assert!(code.contains("/tmp/chart_data.json"));
    }

    #[test]
    fn test_build_chart_python_contains_all_types() {
        let code = build_chart_python("bar", "Test", "/tmp/d.json", "/tmp/o.json", "/tmp/c.html");
        assert!(code.contains("chart_type == 'bar'"));
        assert!(code.contains("chart_type == 'line'"));
        assert!(code.contains("chart_type == 'scatter'"));
        assert!(code.contains("chart_type == 'box'"));
        assert!(code.contains("chart_type == 'heatmap'"));
        assert!(code.contains("chart_type == 'pie'"));
        assert!(code.contains("chart_type == 'histogram'"));
    }

    #[test]
    fn test_build_chart_python_self_contained() {
        let code = build_chart_python("line", "Test", "/tmp/d.json", "/tmp/o.json", "/tmp/c.html");
        // Verify HTML is self-contained (plotly.js inlined for offline use)
        assert!(code.contains("include_plotlyjs=True"));
        assert!(code.contains("full_html=True"));
    }
}
