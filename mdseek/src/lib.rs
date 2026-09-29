//! The library half of `mdseek`: sections from a folder, their ranking and
//! verdict, and the text an agent reads. The binary adds only argument parsing,
//! so an evaluation harness ranks exactly what the agent sees ranked.

pub mod rank;
pub mod read;
pub mod render;
pub mod sections;
