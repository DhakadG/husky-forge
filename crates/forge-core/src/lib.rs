//! Husky Forge core: platform-independent image processing engine.
//! source → inspect → decode → transform → encode → metadata → verify → atomic commit.
pub mod color;
pub mod decode;
pub mod develop;
pub mod encode;
#[cfg(feature = "heic")]
pub mod heif;
pub mod history;
pub mod impact;
pub mod inspect;
pub mod job;
pub mod logging;
pub mod meta;
pub mod rules;
pub mod transform;

pub use decode::Source;
pub use develop::RawLook;
pub use encode::Format;
pub use history::History;
pub use impact::Impact;
pub use inspect::Kind;
pub use job::{Control, Event, Item, Mode, Options, Outcome, Plan, output_dir_for, plan, run, run_with};
pub use meta::{Meta, MetaMode};
pub use rules::Rule;
pub use transform::{Lut, Resize};
