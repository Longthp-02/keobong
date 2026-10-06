//! Payments domain: the host's payout account and VietQR transfer instructions.
//! The app never moves money; it only shows players where to transfer.
//! No Axum, sqlx or HTTP types here.

use std::future::Future;

use chrono::{DateTime, Utc};

use crate::auth::UserId;

/// Banks that accept VietQR transfers, as (BIN, short name). Source: the
/// VietQR bank list (api.vietqr.io/v2/banks, `transferSupported = 1`), 2026-10-06.
pub const BANKS: &[(&str, &str)] = &[
    ("970436", "Vietcombank"),
    ("970415", "VietinBank"),
    ("970418", "BIDV"),
    ("970405", "Agribank"),
    ("970422", "MBBank"),
    ("970407", "Techcombank"),
    ("970416", "ACB"),
    ("970432", "VPBank"),
    ("970423", "TPBank"),
    ("970403", "Sacombank"),
    ("970437", "HDBank"),
    ("970441", "VIB"),
    ("970443", "SHB"),
    ("970431", "Eximbank"),
    ("970426", "MSB"),
    ("970448", "OCB"),
    ("970440", "SeABank"),
    ("970449", "LPBank"),
    ("970454", "VietCapitalBank"),
    ("970429", "SCB"),
    ("546034", "CAKE"),
    ("546035", "Ubank"),
    ("963388", "Timo"),
    ("971025", "MoMo"),
    ("970400", "SaigonBank"),
    ("970409", "BacABank"),
    ("971133", "PVcomBank Pay"),
    ("970412", "PVcomBank"),
    ("970414", "MBV"),
    ("970419", "NCB"),
    ("970424", "ShinhanBank"),
    ("970425", "ABBANK"),
    ("970427", "VietABank"),
    ("970428", "NamABank"),
    ("970430", "PGBank"),
    ("970433", "VietBank"),
    ("970438", "BaoVietBank"),
    ("970446", "COOPBANK"),
    ("970452", "KienLongBank"),
    ("668888", "KBank"),
    ("422589", "CIMB"),
    ("970457", "Woori"),
];

pub fn bank_name(bin: &str) -> Option<&'static str> {
    BANKS.iter().find(|(b, _)| *b == bin).map(|(_, name)| *name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayoutField {
    BankBin,
    AccountNumber,
    AccountName,
}

/// Where players transfer money for a host's matches. Sensitive: shown only to
/// players holding a place in that host's match, never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct PayoutAccount {
    pub bank_bin: String,
    pub account_number: String,
    pub account_name: String,
}

impl std::fmt::Debug for PayoutAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PayoutAccount")
            .field("bank_bin", &self.bank_bin)
            .finish_non_exhaustive()
    }
}

impl PayoutAccount {
    /// Account names are the bank's uppercase name without diacritics; the
    /// web form converts what the host types.
    pub fn validate(
        bank_bin: &str,
        account_number: &str,
        account_name: &str,
    ) -> Result<Self, PayoutField> {
        if bank_name(bank_bin).is_none() {
            return Err(PayoutField::BankBin);
        }
        let number = account_number.trim();
        if !(6..=19).contains(&number.len()) || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(PayoutField::AccountNumber);
        }
        let name = account_name
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !(2..=50).contains(&name.len())
            || !name.bytes().all(|b| b.is_ascii_uppercase() || b == b' ')
        {
            return Err(PayoutField::AccountName);
        }
        Ok(Self {
            bank_bin: bank_bin.to_owned(),
            account_number: number.to_owned(),
            account_name: name,
        })
    }
}

/// What a player needs to pay the host.
#[derive(Clone, PartialEq, Eq)]
pub struct PaymentInstructions {
    pub bank_name: &'static str,
    pub account_number: String,
    pub account_name: String,
    pub amount_vnd: i64,
    pub memo: String,
    /// VietQR (EMVCo) payload; banking apps scan it to prefill the transfer.
    pub qr_payload: String,
}

impl std::fmt::Debug for PaymentInstructions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaymentInstructions")
            .field("amount_vnd", &self.amount_vnd)
            .finish_non_exhaustive()
    }
}

impl PaymentInstructions {
    pub fn new(account: &PayoutAccount, amount_vnd: i64, memo: String) -> Option<Self> {
        Some(Self {
            bank_name: bank_name(&account.bank_bin)?,
            account_number: account.account_number.clone(),
            account_name: account.account_name.clone(),
            amount_vnd,
            qr_payload: vietqr_payload(
                &account.bank_bin,
                &account.account_number,
                amount_vnd,
                &memo,
            ),
            memo,
        })
    }
}

/// Transfer memo: the payment code identifies the player's party in the
/// host's list. Only letters, digits and a space (share ids may contain `-` or
/// `_`, which some banks strip), and short enough for every bank.
pub fn transfer_memo(payment_code: i64) -> String {
    format!("DAGHEP {payment_code}")
}

fn tlv(id: &str, value: &str) -> String {
    format!("{id}{:02}{value}", value.len())
}

/// CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF), as EMVCo QR requires.
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for byte in data {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Builds a dynamic VietQR payload (NAPAS 247 transfer to an account) with a
/// fixed amount and memo. Inputs are already validated ASCII.
pub fn vietqr_payload(bank_bin: &str, account_number: &str, amount_vnd: i64, memo: &str) -> String {
    let beneficiary = tlv("00", bank_bin) + &tlv("01", account_number);
    let merchant = tlv("00", "A000000727") + &tlv("01", &beneficiary) + &tlv("02", "QRIBFTTA");
    let mut payload = tlv("00", "01")
        + &tlv("01", "12")
        + &tlv("38", &merchant)
        + &tlv("53", "704")
        + &tlv("54", &amount_vnd.to_string())
        + &tlv("58", "VN")
        + &tlv("62", &tlv("08", memo))
        + "6304";
    let crc = crc16_ccitt(payload.as_bytes());
    payload.push_str(&format!("{crc:04X}"));
    payload
}

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("storage unavailable: {0}")]
    Unavailable(String),
    #[error("stored data is invalid: {0}")]
    Corrupt(String),
}

/// Storage port for payout accounts.
pub trait PayoutRepository: Send + Sync {
    fn find(
        &self,
        user: UserId,
    ) -> impl Future<Output = Result<Option<PayoutAccount>, RepoError>> + Send;

    fn save(
        &self,
        user: UserId,
        account: &PayoutAccount,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), RepoError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_standard_check_value() {
        assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
    }

    #[test]
    fn payload_matches_a_published_vietqr_example() {
        // From the vietnam-qr-pay library documentation: ACB, 10,000đ, "Chuyen tien".
        assert_eq!(
            vietqr_payload("970416", "257678859", 10_000, "Chuyen tien"),
            "00020101021238530010A0000007270123000697041601092576788590208QRIBFTTA\
             53037045405100005802VN62150811Chuyen tien630453E6"
        );
    }

    #[test]
    fn memo_fits_every_bank() {
        let memo = transfer_memo(i64::MAX);
        assert_eq!(memo, "DAGHEP 9223372036854775807");
        // Letters, digits and one space: no character a bank might strip.
        assert!(memo.chars().all(|c| c.is_ascii_alphanumeric() || c == ' '));
        assert!(transfer_memo(99_999_999_999_999).len() <= 25);
    }

    #[test]
    fn payout_accounts_are_validated_and_normalized() {
        let account =
            PayoutAccount::validate("970436", " 0123456789 ", " PHAM  TRINH HOANG LONG ").unwrap();
        assert_eq!(account.account_number, "0123456789");
        assert_eq!(account.account_name, "PHAM TRINH HOANG LONG");

        assert_eq!(
            PayoutAccount::validate("123456", "0123456789", "LONG"),
            Err(PayoutField::BankBin)
        );
        for number in ["12345", "12345678901234567890", "01234-5678", ""] {
            assert_eq!(
                PayoutAccount::validate("970436", number, "LONG"),
                Err(PayoutField::AccountNumber),
                "{number}"
            );
        }
        for name in ["Long", "PHẠM LONG", "L", &"A".repeat(51), "LONG1"] {
            assert_eq!(
                PayoutAccount::validate("970436", "0123456789", name),
                Err(PayoutField::AccountName),
                "{name}"
            );
        }
    }

    #[test]
    fn bank_details_never_appear_in_debug_output() {
        let account = PayoutAccount::validate("970436", "0123456789", "PHAM LONG").unwrap();
        let instructions = PaymentInstructions::new(&account, 50_000, transfer_memo(7)).unwrap();

        for debug in [format!("{account:?}"), format!("{instructions:?}")] {
            assert!(!debug.contains("0123456789"), "{debug}");
            assert!(!debug.contains("PHAM LONG"), "{debug}");
        }
    }

    #[test]
    fn instructions_carry_a_scannable_payload() {
        let account = PayoutAccount::validate("970416", "257678859", "PHAM LONG").unwrap();

        let instructions = PaymentInstructions::new(&account, 50_000, transfer_memo(7)).unwrap();

        assert_eq!(instructions.bank_name, "ACB");
        assert_eq!(instructions.memo, "DAGHEP 7");
        assert_eq!(
            instructions.qr_payload,
            vietqr_payload("970416", "257678859", 50_000, "DAGHEP 7")
        );
    }
}
