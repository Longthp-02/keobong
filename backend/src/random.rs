//! OS randomness shared by all features.

#[derive(Debug, thiserror::Error)]
#[error("random source unavailable: {0}")]
pub struct RandomnessError(String);

/// Fills `N` bytes from the operating system's secure random source.
pub fn random_bytes<const N: usize>() -> Result<[u8; N], RandomnessError> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|e| RandomnessError(e.to_string()))?;
    Ok(bytes)
}
