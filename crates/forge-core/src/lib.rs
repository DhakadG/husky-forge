//! Husky Forge core: platform-independent image processing engine.
//! source → inspect → decode → transform → encode → metadata → verify → atomic commit.
pub mod decode;
pub mod encode;
pub mod impact;
pub mod inspect;
pub mod job;
pub mod meta;

pub use decode::Source;
pub use encode::Format;
pub use impact::Impact;
pub use inspect::Kind;
pub use job::{Event, Mode, Options, Outcome, Plan, plan, run};
pub use meta::{Meta, MetaMode};
