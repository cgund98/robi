//! Loop tunables.

/// The limits a turn runs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopConfig {
    /// Model turns per run. Not tool calls: fan-out inside one turn is free, and
    /// the cap exists to stop a model that calls tools forever.
    pub max_iterations: u32,
    /// In-flight calls within one batch. A solo call occupies one slot.
    pub max_concurrent_tools: usize,
    /// Run every call alone, in model order, for a reproducible run.
    ///
    /// Overrides a tool's concurrency declaration rather than the segment walk,
    /// so execution stays in model order and only the parallelism goes away.
    pub serial_tools: bool,
    /// Backstop on one serialized tool result. See the tool-result ceiling.
    pub max_tool_result_bytes: usize,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            max_concurrent_tools: 5,
            serial_tools: false,
            max_tool_result_bytes: 256 * 1024,
        }
    }
}

impl LoopConfig {
    /// A one-call-at-a-time configuration, for reproducing a run.
    pub fn serial() -> Self {
        Self {
            serial_tools: true,
            ..Self::default()
        }
    }

    pub fn with_max_iterations(mut self, max_iterations: u32) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    pub fn with_max_concurrent_tools(mut self, max_concurrent_tools: usize) -> Self {
        self.max_concurrent_tools = max_concurrent_tools;
        self
    }

    pub fn with_serial_tools(mut self, serial_tools: bool) -> Self {
        self.serial_tools = serial_tools;
        self
    }

    pub fn with_max_tool_result_bytes(mut self, max_tool_result_bytes: usize) -> Self {
        self.max_tool_result_bytes = max_tool_result_bytes;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_design_doc() {
        let config = LoopConfig::default();
        assert_eq!(config.max_iterations, 50);
        assert_eq!(config.max_concurrent_tools, 5);
        assert!(!config.serial_tools);
        assert_eq!(config.max_tool_result_bytes, 256 * 1024);
    }
}
