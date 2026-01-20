use serde_json;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct AuctionTiming {
    pub lead_arrived_at: Instant,
    pub stages: Vec<TimingStage>,
}

#[derive(Debug, Clone)]
pub struct TimingStage {
    pub name: String,
    pub started_at: Instant,
    pub completed_at: Option<Instant>,
    pub duration_ms: Option<u64>,
    pub metadata: serde_json::Value,
}

impl AuctionTiming {
    pub fn new() -> Self {
        Self {
            lead_arrived_at: Instant::now(),
            stages: Vec::new(),
        }
    }

    pub fn start_stage(&mut self, name: &str, metadata: serde_json::Value) -> usize {
        let stage = TimingStage {
            name: name.to_string(),
            started_at: Instant::now(),
            completed_at: None,
            duration_ms: None,
            metadata,
        };
        self.stages.push(stage);
        self.stages.len() - 1
    }

    pub fn complete_stage(&mut self, index: usize, metadata: Option<serde_json::Value>) {
        if let Some(stage) = self.stages.get_mut(index) {
            stage.completed_at = Some(Instant::now());
            stage.duration_ms = Some(
                stage
                    .completed_at
                    .unwrap()
                    .duration_since(stage.started_at)
                    .as_millis() as u64,
            );
            if let Some(meta) = metadata {
                stage.metadata = meta;
            }
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.lead_arrived_at.elapsed().as_millis() as u64
    }

    pub fn log_summary(&self, lead_id: &str) {
        let total_ms = self.elapsed_ms();
        tracing::info!(
            "Auction timing summary for lead {}: total={}ms, stages={}",
            lead_id,
            total_ms,
            self.stages.len()
        );
        for (i, stage) in self.stages.iter().enumerate() {
            if let Some(duration) = stage.duration_ms {
                tracing::info!(
                    "  Stage {}: {}={}ms, metadata={:?}",
                    i,
                    stage.name,
                    duration,
                    stage.metadata
                );
            }
        }
    }
}

impl Default for AuctionTiming {
    fn default() -> Self {
        Self::new()
    }
}
