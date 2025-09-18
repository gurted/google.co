use gurtd::crawler::scheduler::HostScheduler;
use std::time::{Duration, Instant};

#[tokio::test]
async fn acquire_polite_honors_crawl_delay() {
    let sched = HostScheduler::new(10, 3);
    let start = Instant::now();
    // Queue three crawls for the same host with a 50ms crawl-delay
    let mut handles = Vec::new();
    for _ in 0..3 {
        let s = sched.clone();
        handles.push(tokio::spawn(async move {
            let (_g, _h) = s
                .acquire_polite("delay.test", Some(Duration::from_millis(50)))
                .await;
            // simulate very short fetch
            tokio::time::sleep(Duration::from_millis(1)).await;
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    let elapsed = start.elapsed();
    // With 3 requests and 50ms enforced gap, expect at least ~100ms total
    assert!(
        elapsed.as_millis() >= 90,
        "elapsed too small: {:?}",
        elapsed
    );
}

#[tokio::test]
async fn acquire_polite_fast_when_no_delay() {
    let sched = HostScheduler::new(10, 3);
    let start = Instant::now();
    let mut handles = Vec::new();
    for _ in 0..5 {
        let s = sched.clone();
        handles.push(tokio::spawn(async move {
            let (_g, _h) = s.acquire_polite("fast.test", None).await;
            tokio::time::sleep(Duration::from_millis(1)).await;
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    let elapsed = start.elapsed();
    // Should complete quickly (<40ms) since no enforced delay
    assert!(elapsed.as_millis() < 40, "elapsed too large: {:?}", elapsed);
}
