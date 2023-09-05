use std::time::Duration;

use yield_progress::YieldProgress;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let p = yield_progress::builder()
        .yield_using(tokio::task::yield_now)
        .progress_using(progress_bar())
        .build();

    let task_handle = tokio::task::spawn(cancellable_task(p));

    // Either wait for task to complete or cancel it.
    {
        let ctrlc = tokio::signal::ctrl_c();
        let abort_handle = task_handle.abort_handle();
        tokio::select! {
            _ = ctrlc => {
                abort_handle.abort()
            }
            _ = task_handle => {
                // Task is complete; drop the ctrlc watcher
            }
        }
    }

    println!("Exiting main().");
}

async fn cancellable_task(p: YieldProgress) {
    let length = 1000;
    for i in 0..length {
        p.progress(i as f32 / length as f32).await;
        tokio::time::sleep(Duration::from_micros(100)).await;
    }
    p.finish().await;
}

fn progress_bar() -> impl Fn(f32, &str) {
    // TODO: print label; don't print again if the output would be identical text
    move |fraction, _label| {
        let width = (fraction * 80.0) as usize;
        print!("\r{blank:X<width$}", blank = "");
    }
}
