//! Batches and undo/redo.
//! Arms of `Engine::run`; each works on the model being edited (inside `edit`, the open component's definition).

use super::*;

impl Engine {
    pub(super) fn run_session(&mut self, cmd: Command) -> Result<Value> {
        Ok(match cmd {
            Command::BeginBatch => {
                if self.batch.is_some() {
                    self.end_batch();
                }
                self.batch = Some((self.model.clone(), self.revision, std::time::Instant::now()));
                self.batch_seq += 1;
                json!({"batch": self.batch_seq})
            }
            Command::EndBatch { id } => {
                // an end that names a batch that is already over (closed by the timer, or replaced) leaves the current one alone
                if id.is_none_or(|i| i == self.batch_seq) {
                    self.last_batch = None;
                    self.end_batch();
                }
                // a plugin's run that changed the model: who, and how many things (the app says so, with Undo)
                match self.last_batch.take() {
                    Some((who, n)) if n > 0 => json!({"origin": who, "changed": n}),
                    _ => json!({}),
                }
            }
            Command::Undo => {
                if self.batch.is_some() {
                    bail!("a plugin is still running; Undo works when it has finished");
                }
                self.step_history(true)?
            }
            Command::Redo => {
                if self.batch.is_some() {
                    bail!("a plugin is still running; Redo works when it has finished");
                }
                self.step_history(false)?
            }
            other => unreachable!("command routed to the wrong group: {other:?}"),
        })
    }
}
