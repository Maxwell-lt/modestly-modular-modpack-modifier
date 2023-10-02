use chrono::{DateTime, Utc};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogMessage {
    pub source: String,
    pub level: LogLevel,
    pub message: String,
    pub data: Option<Vec<String>>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Panic,
}

#[derive(Debug, Clone)]
pub struct Logger {
    logs: Arc<boxcar::Vec<LogMessage>>,
}

impl Logger {
    pub fn new() -> Self {
        Logger {
            logs: Arc::new(boxcar::Vec::new()),
        }
    }

    pub fn log(&self, source: String, level: LogLevel, message: String, data: Option<Vec<String>>) {
        self.logs.push(LogMessage {
            source,
            level,
            message,
            data,
            timestamp: Utc::now(),
        });
    }

    pub fn get_logs(&self) -> impl Iterator<Item = &LogMessage> + '_ {
        self.logs.iter().map(|item| item.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logger() {
        let logger = Logger::new();

        logger.log("my-node".into(), LogLevel::Info, "Did a thing".into(), None);

        let mut log_iter = logger.get_logs();
        let next = log_iter.next().unwrap();
        assert_eq!(next.data, None);
        assert_eq!(next.source, "my-node");
        assert_eq!(next.level, LogLevel::Info);
        assert_eq!(next.message, "Did a thing");

        assert_eq!(log_iter.next(), None);
    }
}
