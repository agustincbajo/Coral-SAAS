//! Worker library surface. The binary (`main.rs`) drives the queue loop;
//! everything that actually runs a job lives here so integration tests
//! can exercise the pipeline without Redis/Postgres.

pub mod context;
pub mod coral_runner;
