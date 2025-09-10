use std::time::Instant;
use gurtd::crawler::scheduler::HostScheduler;

#[tokio::test]
async fn per_host_limit_enforced() {
    let sched = HostScheduler::new(10, 2);
    let start = Instant::now();
    let mut handles = Vec::new();
    for _ in 0..5 {
        let s = sched.clone();
        handles.push(tokio::spawn(async move {
            let (_g, _h) = s.acquire("a.test").await;
            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        }));
    }
    for h in handles { h.await.unwrap(); }
    let elapsed = start.elapsed();
    // With limit=2 and 5 tasks at 60ms each, expect >= 3 waves: >= 180ms
    assert!(elapsed.as_millis() >= 170, "elapsed too small: {:?}", elapsed);
}

#[tokio::test]
async fn global_limit_enforced() {
    let sched = HostScheduler::new(3, 10);
    let start = Instant::now();
    let mut handles = Vec::new();
    for i in 0..9 {
        let s = sched.clone();
        handles.push(tokio::spawn(async move {
            let host = if i % 3 == 0 { "a" } else if i % 3 == 1 { "b" } else { "c" };
            let (_g, _h) = s.acquire(host).await;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }));
    }
    for h in handles { h.await.unwrap(); }
    let elapsed = start.elapsed();
    // With global limit 3 and 9 tasks of 50ms, expect >= 150ms
    assert!(elapsed.as_millis() >= 140, "elapsed too small: {:?}", elapsed);
}

