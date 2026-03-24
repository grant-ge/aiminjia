//! Benchmark: chromiumoxide 0.7
//! Tests: launch, navigate, evaluate, read DOM, close

use chromiumoxide_old::browser::{Browser, BrowserConfig};
use futures::StreamExt;
use std::time::Instant;

const CHROME_PATH: &str = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const TEST_URLS: &[&str] = &[
    "https://example.com",
    "https://httpbin.org/html",
    "https://jsonplaceholder.typicode.com",
];

const EXTRACT_JS: &str = r#"(() => {
    const tables = [];
    document.querySelectorAll('table').forEach((t, i) => {
        if (i >= 5) return;
        const headers = [...t.querySelectorAll('thead th, tr:first-child th')].map(h => h.innerText.trim());
        const rows = [];
        t.querySelectorAll('tbody tr, tr').forEach((tr, ri) => {
            if (ri === 0 && headers.length > 0) return;
            if (rows.length >= 50) return;
            const cells = [...tr.querySelectorAll('td')].map(c => c.innerText.trim());
            rows.push(cells);
        });
        tables.push({ headers, rows });
    });
    const el = document.querySelector('main') || document.body;
    return {
        url: location.href,
        title: document.title,
        tables,
        text: (el ? el.innerText : '').substring(0, 4000),
    };
})()"#;

#[tokio::main]
async fn main() {
    println!("=== chromiumoxide 0.7 benchmark ===\n");

    // 1. Launch
    let t0 = Instant::now();
    let config = BrowserConfig::builder()
        .chrome_executable(CHROME_PATH)
        .with_head()
        .viewport(None)
        .arg("--disable-extensions")
        .arg("--no-first-run")
        .arg("--disable-blink-features=AutomationControlled")
        .build()
        .expect("config");

    let (mut browser, mut handler) = Browser::launch(config).await.expect("launch");
    tokio::spawn(async move { while let Some(_) = handler.next().await {} });
    let launch_ms = t0.elapsed().as_millis();
    println!("[1] Launch:          {} ms", launch_ms);

    // 2. Create page
    let t1 = Instant::now();
    let page = browser.new_page("about:blank").await.expect("new_page");
    let new_page_ms = t1.elapsed().as_millis();
    println!("[2] New page:        {} ms", new_page_ms);

    // 3. Navigate to each URL
    for url in TEST_URLS {
        let t = Instant::now();
        page.goto(*url).await.expect("goto");
        let nav_ms = t.elapsed().as_millis();

        // 4. Simple evaluate
        let t2 = Instant::now();
        let _title: String = page.evaluate("document.title")
            .await.expect("eval")
            .into_value().unwrap_or_default();
        let eval_ms = t2.elapsed().as_millis();

        // 5. Heavy extract (simulate read_content)
        let t3 = Instant::now();
        let _data: serde_json::Value = page.evaluate(EXTRACT_JS)
            .await.expect("extract")
            .into_value().unwrap_or(serde_json::Value::Null);
        let extract_ms = t3.elapsed().as_millis();

        println!("[3] Navigate {:<42} {} ms", url, nav_ms);
        println!("    eval(title):     {} ms", eval_ms);
        println!("    extract(full):   {} ms", extract_ms);
    }

    // 6. Rapid-fire evaluates (10x) to test round-trip latency
    let t4 = Instant::now();
    for _ in 0..10 {
        let _: i32 = page.evaluate("1+1").await.expect("rapid")
            .into_value().unwrap_or(0);
    }
    let rapid_ms = t4.elapsed().as_millis();
    println!("\n[4] 10x evaluate(1+1): {} ms  (avg {} ms/call)", rapid_ms, rapid_ms / 10);

    // 7. Liveness check pattern (simulate get_active_page)
    let t5 = Instant::now();
    for _ in 0..10 {
        let _: i32 = page.evaluate("1").await.expect("liveness")
            .into_value().unwrap_or(0);
    }
    let liveness_ms = t5.elapsed().as_millis();
    println!("[5] 10x liveness(1):   {} ms  (avg {} ms/call)", liveness_ms, liveness_ms / 10);

    // 8. Close
    let t6 = Instant::now();
    let _ = page.close().await;
    let _ = browser.close().await;
    let close_ms = t6.elapsed().as_millis();
    println!("\n[6] Close:           {} ms", close_ms);

    println!("\n=== Total (excl launch): {} ms ===",
        new_page_ms + rapid_ms + liveness_ms + close_ms);
}
