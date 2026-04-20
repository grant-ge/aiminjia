//! generate_chart handler — Plotly interactive HTML charts.

use std::path::Path;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::FileMeta;
use crate::runtime::tools::builtin::chart_capability::{ChartCapability, DefaultChartCapability};

use super::require_str;
use super::FileGenResult;

pub(crate) struct ChartCoreParams<'a> {
    pub workspace_path: &'a Path,
    pub conversation_id: &'a str,
}

/// 5. generate_chart — create a Plotly interactive HTML chart.
pub(crate) async fn handle_generate_chart(
    ctx: &PluginContext,
    args: &Value,
) -> Result<FileGenResult> {
    let (python_binary, python_home) =
        crate::python::runner::resolve_python_path(ctx.app_handle.as_ref());
    let capability = DefaultChartCapability {
        storage: ctx.storage.clone(),
        workspace_path: ctx.workspace_path.clone(),
        python_binary,
        python_home,
    };
    let params = ChartCoreParams {
        workspace_path: &ctx.workspace_path,
        conversation_id: &ctx.conversation_id,
    };
    handle_generate_chart_core(&params, args, &capability).await
}

pub(crate) async fn handle_generate_chart_core(
    params: &ChartCoreParams<'_>,
    args: &Value,
    capability: &dyn ChartCapability,
) -> Result<FileGenResult> {
    let chart_type = require_str(args, "chart_type")?;
    let title = require_str(args, "title")?;
    let data = resolve_chart_data(params.workspace_path, args)?;
    let options = args.get("options").cloned().unwrap_or(json!({}));

    let generated = capability
        .run_chart_python(params.workspace_path, chart_type, title, &data, &options)
        .await?;
    let persisted = capability
        .persist_chart(
            params.conversation_id,
            &generated.html_bytes,
            &generated.chart_filename,
            chart_type,
            title,
        )
        .await?;

    let content = serde_json::to_string_pretty(&json!({
        "fileId": persisted.file_id,
        "fileName": persisted.file_name,
        "storedPath": persisted.stored_path,
        "fileSize": persisted.file_size,
        "chartType": chart_type,
    }))?;

    Ok(FileGenResult {
        content,
        file_meta: FileMeta {
            file_id: persisted.file_id,
            file_name: persisted.file_name,
            requested_format: "html".to_string(),
            actual_format: "html".to_string(),
            file_size: persisted.file_size,
            stored_path: persisted.stored_path,
            category: "chart".to_string(),
        },
        is_degraded: false,
        degradation_notice: None,
    })
}

fn resolve_chart_data(workspace_path: &Path, args: &Value) -> Result<Value> {
    if let Some(data_file_path) = args.get("data_file").and_then(|v| v.as_str()) {
        let full_path = if std::path::Path::new(data_file_path).is_absolute() {
            std::path::PathBuf::from(data_file_path)
        } else {
            workspace_path.join(data_file_path)
        };
        let canonical = full_path.canonicalize().map_err(|e| {
            anyhow!(
                "Failed to read data_file '{}': {}. Use execute_python to generate the JSON file first.",
                data_file_path,
                e
            )
        })?;
        let workspace_canonical = workspace_path
            .canonicalize()
            .unwrap_or_else(|_| workspace_path.to_path_buf());
        if !canonical.starts_with(&workspace_canonical) {
            return Err(anyhow!(
                "data_file path '{}' is outside the workspace directory. Only files within the workspace are allowed.",
                data_file_path
            ));
        }
        let content = std::fs::read_to_string(&canonical).map_err(|e| {
            anyhow!(
                "Failed to read data_file '{}': {}. Use execute_python to generate the JSON file first.",
                data_file_path,
                e
            )
        })?;
        let data = serde_json::from_str(&content).map_err(|e| {
            anyhow!(
                "Failed to parse chart data from '{}': {}",
                data_file_path,
                e
            )
        })?;
        log::info!("[generate_chart] Loaded data from file: {}", data_file_path);
        Ok(data)
    } else if let Some(inline_data) = args.get("data") {
        Ok(inline_data.clone())
    } else {
        Err(anyhow!(
            "Missing chart data. Provide either:\n\
             1. 'data_file': path to a JSON file with chart data (recommended for large datasets)\n\
             2. 'data': inline chart data object"
        ))
    }
}

pub(crate) fn build_chart_python(
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

# Support custom key mapping via options (for flexible data formats)
x_key = options.get('x_key', 'labels')
y_keys = options.get('y_keys', None)  # List of keys for multi-series, or None for auto-detect

labels = data.get(x_key, [])

# Resolve values: try y_keys first, then 'values', then 'datasets'
if y_keys:
    # Multi-series: y_keys=['new_med', 'old_med'] → values=[[...], [...]]
    if len(y_keys) == 1:
        values = data.get(y_keys[0], [])
    else:
        values = [data.get(k, []) for k in y_keys]
        # Use y_labels from options if provided, otherwise use y_keys as labels
        if not data.get('series_names'):
            data['series_names'] = options.get('y_labels', y_keys)
else:
    values = data.get('values', [])
    # Support 'datasets' format: [{{"label": "...", "values": [...]}}]
    datasets = data.get('datasets', [])
    if not values and datasets:
        if len(datasets) == 1:
            values = datasets[0].get('values', [])
        else:
            values = [ds.get('values', []) for ds in datasets]
            if not data.get('series_names'):
                data['series_names'] = [ds.get('label', f'Series {{{{i+1}}}}') for i, ds in enumerate(datasets)]

fig = go.Figure()

if chart_type == 'bar':
    if isinstance(values[0], list) if values else False:
        series_names = data.get('series_names', [f'Series {{i+1}}' for i in range(len(values))])
        for i, v in enumerate(values):
            name = series_names[i] if i < len(series_names) else f'Series {{i+1}}'
            fig.add_trace(go.Bar(x=labels, y=v, name=name))
        fig.update_layout(barmode=options.get('barmode', 'group'))
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

elif chart_type == 'nine_box':
    # 9-box talent grid (3x3 matrix of perf x potential).
    #
    # Accepts data['grid'] = {{ '中文标签': {{
    #     'count': N, 'percentage': P,
    #     'perf_level': 1|2|3, 'pot_level': 1|2|3,
    #     'label_en': 'English label' (optional)
    # }} }}
    # — exactly the shape produced by talent-9box scripts/step2.py.
    #
    # Visual: chart top = perf high; chart left = pot low (HR convention).
    # Colors are 4-tier health, NOT magnitude — green = top performers,
    # red = at-risk. Cell text shows label + count + %.
    grid = data.get('grid', {{}})

    # Health tier mapping (1=red, 2=orange, 3=yellow, 4=green)
    def _tier(perf, pot):
        if perf == 3 and pot == 3:                   return 4  # Star
        if (perf, pot) in [(3, 2), (2, 3)]:          return 4  # Core / Rising
        if (perf, pot) in [(3, 1), (2, 2)]:          return 3  # Expert / Steady
        if (perf, pot) in [(2, 1), (1, 3), (1, 2)]:  return 2  # Watch list / dev
        if perf == 1 and pot == 1:                   return 1  # Underperformer
        return 2

    z = [[0]*3 for _ in range(3)]
    text_grid = [['']*3 for _ in range(3)]
    hover_grid = [['']*3 for _ in range(3)]

    for label, cell in grid.items():
        try:
            perf = int(cell.get('perf_level', 0))
            pot = int(cell.get('pot_level', 0))
        except (TypeError, ValueError):
            continue
        if not (1 <= perf <= 3 and 1 <= pot <= 3):
            continue
        row_idx = 3 - perf  # perf 3 → row 0 (top of chart)
        col_idx = pot - 1   # pot 1 → col 0 (left of chart)
        z[row_idx][col_idx] = _tier(perf, pot)
        cnt = cell.get('count', 0)
        pct = cell.get('percentage', 0)
        try:
            pct_str = f"{{pct:.1f}}"
        except (TypeError, ValueError):
            pct_str = str(pct)
        text_grid[row_idx][col_idx] = f"<b>{{label}}</b><br>{{cnt}} 人 ({{pct_str}}%)"
        en = cell.get('label_en', '')
        hover_grid[row_idx][col_idx] = (
            f"{{label}} ({{en}})<br>{{cnt}} 人，占比 {{pct_str}}%"
            if en else f"{{label}}<br>{{cnt}} 人，占比 {{pct_str}}%"
        )

    # Discrete 4-tier colorscale (light pastels for readability with text).
    # Tier values 1..4 → normalized 0, 0.333, 0.667, 1.0 (zmin=1, zmax=4).
    # Boundaries at midpoints 0.167, 0.5, 0.833 give clean step transitions.
    colorscale = [
        [0.000, '#ffcdd2'], [0.167, '#ffcdd2'],  # tier 1 — red
        [0.167, '#ffccbc'], [0.500, '#ffccbc'],  # tier 2 — orange
        [0.500, '#fff9c4'], [0.833, '#fff9c4'],  # tier 3 — yellow
        [0.833, '#c8e6c9'], [1.000, '#c8e6c9'],  # tier 4 — green
    ]

    fig.add_trace(go.Heatmap(
        z=z,
        x=['潜力 低', '潜力 中', '潜力 高'],
        y=['绩效 高', '绩效 中', '绩效 低'],
        text=text_grid,
        texttemplate='%{{text}}',
        textfont=dict(size=13, color='#212121'),
        hovertext=hover_grid,
        hovertemplate='%{{hovertext}}<extra></extra>',
        colorscale=colorscale,
        showscale=False,
        zmin=1, zmax=4,
        xgap=4, ygap=4,
    ))

    # Square aspect, no axis lines/ticks (cell labels are in text)
    fig.update_xaxes(side='top', showgrid=False, zeroline=False, fixedrange=True)
    fig.update_yaxes(showgrid=False, zeroline=False, fixedrange=True, autorange='reversed')

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
        let code = build_chart_python(
            "bar",
            "My Chart",
            "/tmp/chart_data.json",
            "/tmp/chart_opts.json",
            "/tmp/chart.html",
        );
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
        assert!(code.contains("chart_type == 'nine_box'"));
    }

    #[test]
    fn test_build_chart_python_nine_box_renders_grid() {
        let code = build_chart_python("nine_box", "九宫格", "/tmp/d.json", "/tmp/o.json", "/tmp/c.html");
        // Nine-box branch must read data['grid'] and emit a Heatmap
        assert!(code.contains("data.get('grid'"));
        assert!(code.contains("go.Heatmap"));
        // Y axis must put '绩效 高' first (autorange='reversed' makes it top of chart)
        assert!(code.contains("绩效 高"));
        assert!(code.contains("潜力 低"));
        // Tier health-color scale (4 discrete tiers, must contain green for stars)
        assert!(code.contains("#c8e6c9"));
        assert!(code.contains("#ffcdd2"));
        // Boundary check: 0.167 / 0.5 / 0.833 (clean step transitions)
        assert!(code.contains("0.167"));
        assert!(code.contains("0.500"));
        assert!(code.contains("0.833"));
    }

    #[test]
    fn test_build_chart_python_self_contained() {
        let code = build_chart_python("line", "Test", "/tmp/d.json", "/tmp/o.json", "/tmp/c.html");
        // Verify HTML is self-contained (plotly.js inlined for offline use)
        assert!(code.contains("include_plotlyjs=True"));
        assert!(code.contains("full_html=True"));
    }
}
