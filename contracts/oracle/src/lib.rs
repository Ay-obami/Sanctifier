#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Vec,
};

#[cfg(test)]
mod test;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    InvalidThreshold = 3,
    InsufficientSigners = 4,
    StalePrice = 5,
}

#[contracttype]
pub enum DataKey {
    Validators,
    Threshold,
    PriceData,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PriceData {
    pub price: u128,
    pub timestamp: u64,
}

/// Event emitted by heartbeat() to allow monitoring systems to track feeder health.
#[contracttype]
#[derive(Clone, Debug)]
pub struct HeartbeatEvent {
    pub last_update: u64,
    pub current_time: u64,
    pub time_since_update: u64,
}

#[contract]
pub struct OracleContract;

#[contractimpl]
impl OracleContract {
    /// Initialize the oracle with a set of trusted validators and a consensus threshold.
    pub fn init(env: Env, validators: Vec<Address>, threshold: u32) {
        if env.storage().instance().has(&DataKey::Threshold) {
            env.panic_with_error(Error::AlreadyInitialized);
        }
        if threshold == 0 || threshold > validators.len() {
            env.panic_with_error(Error::InvalidThreshold);
        }
        env.storage()
            .instance()
            .set(&DataKey::Validators, &validators);
        env.storage()
            .instance()
            .set(&DataKey::Threshold, &threshold);
    }

    /// Update the price feed. Requires multi-sig authorization from validators.
    pub fn update_price(env: Env, price: u128, timestamp: u64, validators_approving: Vec<Address>) {
        let trusted_validators: Vec<Address> =
            env.storage().instance().get(&DataKey::Validators).unwrap();
        let threshold: u32 = env.storage().instance().get(&DataKey::Threshold).unwrap();

        let mut valid_count = 0;
        let mut processed = Vec::<Address>::new(&env);

        for validator in validators_approving {
            if trusted_validators.contains(&validator) && !processed.contains(&validator) {
                validator.require_auth();
                valid_count += 1;
                processed.push_back(validator);
            }
        }

        if valid_count < threshold {
            env.panic_with_error(Error::InsufficientSigners);
        }

        env.storage()
            .instance()
            .set(&DataKey::PriceData, &PriceData { price, timestamp });

        env.events()
            .publish((symbol_short!("price_upd"),), (price, timestamp));
    }

    /// Read the latest price. Validates that the price is not older than max_age.
    pub fn get_price(env: Env, max_age: u64) -> u128 {
        let data: PriceData = env
            .storage()
            .instance()
            .get(&DataKey::PriceData)
            .unwrap_or_else(|| {
                env.panic_with_error(Error::StalePrice);
            });

        let current_time = env.ledger().timestamp();
        if current_time > data.timestamp + max_age {
            env.panic_with_error(Error::StalePrice);
        }

        data.price
    }

    /// Heartbeat function for monitoring the oracle's health.
    /// Emits an event containing the last update timestamp and time elapsed since last update.
    /// 
    /// Monitoring systems should call this periodically to:
    /// - Track if the price feeder is still active
    /// - Alert when time_since_update exceeds acceptable thresholds
    /// - Verify the oracle is operational
    ///
    /// # Monitoring Integration
    /// 
    /// Example monitoring setup:
    /// 1. Call `heartbeat()` every 60 seconds
    /// 2. Listen for `heartbeat` events
    /// 3. Alert if `time_since_update` > expected feed interval
    /// 4. Alert if no `heartbeat` events received for N intervals
    ///
    /// # Returns
    /// Emits a `HeartbeatEvent` containing:
    /// - `last_update`: timestamp of the last price update
    /// - `current_time`: current ledger timestamp
    /// - `time_since_update`: seconds elapsed since last update
    pub fn heartbeat(env: Env) {
        let data: Option<PriceData> = env.storage().instance().get(&DataKey::PriceData);
        let current_time = env.ledger().timestamp();

        match data {
            Some(price_data) => {
                let time_since_update = current_time.saturating_sub(price_data.timestamp);
                env.events().publish(
                    (symbol_short!("heartbeat"),),
                    HeartbeatEvent {
                        last_update: price_data.timestamp,
                        current_time,
                        time_since_update,
                    },
                );
            }
            None => {
                // No price data yet - emit heartbeat with zero last_update
                env.events().publish(
                    (symbol_short!("heartbeat"),),
                    HeartbeatEvent {
                        last_update: 0,
                        current_time,
                        time_since_update: current_time,
                    },
                );
            }
        }
    }

    /// Check if the oracle is alive (has recent data).
    /// 
    /// Returns `true` if the oracle has price data that is newer than `max_age` seconds,
    /// `false` otherwise.
    ///
    /// This is a view function that allows contracts and monitoring systems to check
    /// feeder health without reverting.
    ///
    /// # Arguments
    /// - `max_age`: Maximum acceptable age in seconds for the last price update
    ///
    /// # Returns
    /// - `true` if oracle has data and it's fresh (age <= max_age)
    /// - `false` if oracle has no data or data is stale (age > max_age)
    ///
    /// # Example
    /// ```ignore
    /// // Check if oracle has data from the last hour (3600 seconds)
    /// if oracle.is_alive(3600) {
    ///     let price = oracle.get_price(3600);
    ///     // Use price...
    /// } else {
    ///     // Feeder is down, use fallback...
    /// }
    /// ```
    pub fn is_alive(env: Env, max_age: u64) -> bool {
        let data: Option<PriceData> = env.storage().instance().get(&DataKey::PriceData);

        match data {
            Some(price_data) => {
                let current_time = env.ledger().timestamp();
                let age = current_time.saturating_sub(price_data.timestamp);
                age <= max_age
            }
            None => false,
        }
    }
}
