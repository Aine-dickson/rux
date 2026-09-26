//! A shop's catalogue and till, in Rust.
//!
//! `Product` is a value: Rux gets a copy, can read and build one, and calls
//! its methods. `Till` is a resource: Rux holds it and calls its methods, and
//! never sees the money inside, which is kept in whole cents so no float ever
//! adds up wrong.

use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

#[rux::export]
pub struct Product {
    pub id: u32,
    pub name: String,
    pub price: f64,
    pub note: Option<String>,
}

#[rux::export]
impl Product {
    /// The price with `rate` of tax on it, to the cent.
    pub fn with_tax(&self, rate: f64) -> f64 {
        (self.price * (1.0 + rate) * 100.0).round() / 100.0
    }
}

/// What the shop sells.
#[rux::export]
pub fn catalogue() -> Vec<Product> {
    vec![
        Product { id: 1, name: "Tea".into(), price: 2.4, note: None },
        Product { id: 2, name: "Bun".into(), price: 1.75, note: Some("baked today".into()) },
        Product { id: 3, name: "Jam".into(), price: 4.1, note: None },
    ]
}

#[rux::resource]
pub struct Till {
    cents: Mutex<i64>,
    count: Mutex<u32>,
}

/// Why a till refuses.
#[derive(Debug)]
pub struct TillError(String);

impl fmt::Display for TillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[rux::export]
impl Till {
    /// Ring `p` up; gives the number of items rung so far.
    pub fn ring(&self, p: Product) -> Result<u32, TillError> {
        if p.price <= 0.0 {
            return Err(TillError(format!("{} has no price", p.name)));
        }
        *self.cents.lock().unwrap() += (p.price * 100.0).round() as i64;
        let mut count = self.count.lock().unwrap();
        *count += 1;
        Ok(*count)
    }

    pub fn total(&self) -> f64 {
        *self.cents.lock().unwrap() as f64 / 100.0
    }

    /// Settle with the bank: slow, so it is `async`, and it runs off the UI
    /// thread. An empty till is refused.
    pub async fn settle(&self) -> Result<String, TillError> {
        std::thread::sleep(Duration::from_millis(800));
        let mut cents = self.cents.lock().unwrap();
        let mut count = self.count.lock().unwrap();
        if *count == 0 {
            return Err(TillError("nothing to settle".into()));
        }
        let done = format!("settled {} items for {:.2}", *count, *cents as f64 / 100.0);
        *cents = 0;
        *count = 0;
        Ok(done)
    }
}

#[rux::export]
pub fn open_till() -> Till {
    Till { cents: Mutex::new(0), count: Mutex::new(0) }
}
