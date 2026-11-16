//! `jobs` — detached background operations (`respond --background --detach`).

use serde_json::Value;

use crate::cli::JobsCmd;
use crate::output::{scalar, Io, EXIT_FAILURE};

pub(crate) async fn run(io: &mut Io<'_>, cmd: JobsCmd) -> u8 {
    match cmd {
        JobsCmd::Status { provider, id } => {
            match lexi_host::operation_status(&provider, &id).await {
                Ok(st) => report(io, &provider, &st),
                Err(e) => io.fail(e),
            }
        }
        JobsCmd::Wait {
            provider,
            id,
            interval,
            timeout,
        } => {
            let interval = interval.max(1);
            let polls = (timeout / interval).max(1) as usize;
            let wait = lexi_host::wait_operation(
                &provider,
                &id,
                std::time::Duration::from_secs(interval),
                polls,
            );
            match wait.await {
                Ok(st) => report(io, &provider, &st),
                Err(e) => io.fail(e),
            }
        }
    }
}

/// `{id}: {state} ({status})`, then the result text once the job succeeded;
/// JSON: the operation plus `provider` and (when done) `text`. A failed job
/// exits 1.
fn report(io: &mut Io<'_>, provider: &str, st: &lexi_host::OperationStatus) -> u8 {
    let op = &st.operation;
    let mut json = op.clone();
    json["provider"] = Value::from(provider);
    let mut human = format!(
        "{}: {} ({})",
        scalar(op.get("id")),
        scalar(op.get("state")),
        scalar(op.get("status"))
    );
    if let Some(res) = &st.result {
        json["text"] = Value::from(res.text.clone());
        human = format!("{human}\n{}", res.text);
    }
    let code = io.result(human, json);
    if st.failed() {
        EXIT_FAILURE
    } else {
        code
    }
}
