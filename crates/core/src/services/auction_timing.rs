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

impl Default for AuctionTiming {
    fn default() -> Self {
        Self {
            lead_arrived_at: Instant::now(),
            stages: Vec::new(),
        }
    }
}

impl AuctionTiming {
    pub fn new() -> Self {
        Self::default()
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
        let stages_json = serde_json::to_string(
            &self
                .stages
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "name": s.name,
                        "duration_ms": s.duration_ms,
                        "metadata": s.metadata
                    })
                })
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".to_string());

        tracing::info!(
            lead_id = %lead_id,
            total_ms = total_ms,
            stage_count = self.stages.len(),
            stages = %stages_json,
            "Auction timing summary"
        );

        // Also log individual stages with structured fields for better queryability
        for (i, stage) in self.stages.iter().enumerate() {
            if let Some(duration) = stage.duration_ms {
                let metadata_json =
                    serde_json::to_string(&stage.metadata).unwrap_or_else(|_| "{}".to_string());
                tracing::info!(
                    lead_id = %lead_id,
                    stage_index = i,
                    stage_name = %stage.name,
                    duration_ms = duration,
                    metadata = %metadata_json,
                    "Auction stage completed"
                );
            }
        }
    }

    pub fn to_json(&self, lead_id: &str) -> serde_json::Value {
        serde_json::json!({
            "lead_id": lead_id,
            "total_ms": self.elapsed_ms(),
            "stages": self.stages.iter().map(|s| serde_json::json!({
                "name": s.name,
                "duration_ms": s.duration_ms,
                "metadata": s.metadata
            })).collect::<Vec<_>>()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auction_timing_new() {
        let timing = AuctionTiming::new();
        assert_eq!(timing.stages.len(), 0);
    }

    #[test]
    fn test_start_stage() {
        let mut timing = AuctionTiming::new();
        let index = timing.start_stage("test_stage", serde_json::json!({}));
        assert_eq!(index, 0);
        assert_eq!(timing.stages.len(), 1);
        assert_eq!(timing.stages[0].name, "test_stage");
    }

    #[test]
    fn test_complete_stage() {
        let mut timing = AuctionTiming::new();
        let index = timing.start_stage("test_stage", serde_json::json!({}));
        std::thread::sleep(std::time::Duration::from_millis(10));
        timing.complete_stage(index, None);
        assert!(timing.stages[0].completed_at.is_some());
        assert!(timing.stages[0].duration_ms.is_some());
        assert!(timing.stages[0].duration_ms.unwrap() >= 10);
    }

    #[test]
    fn test_elapsed_ms() {
        let timing = AuctionTiming::new();
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(timing.elapsed_ms() >= 10);
    }

    #[test]
    fn test_to_json() {
        let mut timing = AuctionTiming::new();
        let index = timing.start_stage("test_stage", serde_json::json!({"key": "value"}));
        timing.complete_stage(index, None);
        let json = timing.to_json("test-lead-id");
        assert!(json.get("lead_id").is_some());
        assert!(json.get("total_ms").is_some());
        assert!(json.get("stages").is_some());
    }
}
