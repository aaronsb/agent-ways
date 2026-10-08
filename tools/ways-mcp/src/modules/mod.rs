//! The modules this server hosts. A module joins by adding itself here
//! (ADR-501 item 3).

mod lookup;
mod ways;

use crate::server::Module;

pub fn all() -> Vec<Box<dyn Module>> {
    vec![Box::new(ways::Ways)]
}
