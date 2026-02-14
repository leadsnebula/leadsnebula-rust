#![allow(dead_code)]
// Rate limiting is implemented in `routes/leads.rs` to enforce the 360/hour policy
// directly at the lead entrypoint where publisher context and response shape are known.
