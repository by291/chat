//! Identity naming rules shared by account creation, profile updates and exact lookup.

use crate::error::AppError;

/// Trim and lowercase an ASCII username; invalid length or characters return invalid-input.
pub fn username(value: &str) -> Result<String, AppError> {
    let normalized = value.trim().to_ascii_lowercase();
    let bytes = normalized.as_bytes();

    if !(3..=32).contains(&bytes.len())
        || !bytes[0].is_ascii_lowercase()
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        return Err(AppError::invalid(
            "用户名须为 3–32 位小写字母、数字或下划线，并以字母开头",
        ));
    }

    Ok(normalized)
}

/// Trim a display name and require 1–64 Unicode scalar values; invalid input is rejected.
pub fn display_name(value: &str) -> Result<String, AppError> {
    let trimmed = value.trim();

    if !(1..=64).contains(&trimmed.chars().count()) {
        return Err(AppError::invalid("昵称需要 1–64 个字符"));
    }

    Ok(trimmed.to_owned())
}
