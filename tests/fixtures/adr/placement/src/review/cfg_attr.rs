#[cfg(test)]
use std::fs;

fn shipped() { println!("ADR-101 ships after a cfg(test) import"); }

#[cfg(test)]
#[allow(unused)]
mod tests {
    fn label() -> &'static str { "ADR-102" }
}

fn after() { println!("ADR-102 ships after the test module"); }
