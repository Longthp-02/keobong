//! Venues organisers add for their groups from a Google Maps link (public
//! groups, `docs/public-groups-plan.md`). They wait for an admin and are never
//! offered for football matches; the launch venues stay as they are.

use std::future::Future;

use sqlx::PgPool;

use super::domain::RepoError;
use crate::auth::UserId;
use crate::random::random_bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sport {
    Football,
    Basketball,
}

impl Sport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Football => "football",
            Self::Basketball => "basketball",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "football" => Some(Self::Football),
            "basketball" => Some(Self::Basketball),
            _ => None,
        }
    }
}

/// How a group plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameFormat {
    ThreeOnThree,
    FiveOnFive,
    FiveASide,
    SevenASide,
    ElevenASide,
}

impl GameFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ThreeOnThree => "3x3",
            Self::FiveOnFive => "5x5",
            Self::FiveASide => "five_a_side",
            Self::SevenASide => "seven_a_side",
            Self::ElevenASide => "eleven_a_side",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        [
            Self::ThreeOnThree,
            Self::FiveOnFive,
            Self::FiveASide,
            Self::SevenASide,
            Self::ElevenASide,
        ]
        .into_iter()
        .find(|format| format.as_str() == raw)
    }

    /// The sport this format belongs to.
    pub fn sport(self) -> Sport {
        match self {
            Self::ThreeOnThree | Self::FiveOnFive => Sport::Basketball,
            _ => Sport::Football,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VenueStatus {
    Pending,
    Approved,
    Rejected,
}

impl VenueStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

/// A venue as groups use it.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupVenue {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub address: String,
    pub sports: Vec<Sport>,
    pub lat: f64,
    pub lng: f64,
}

/// A venue an organiser proposes.
#[derive(Debug, Clone, PartialEq)]
pub struct VenueSubmission {
    pub name: String,
    pub address: String,
    pub lat: f64,
    pub lng: f64,
    pub sports: Vec<Sport>,
    pub maps_url: String,
    pub submitted_by: UserId,
}

/// Longest venue name, in characters (the database allows 120).
pub const MAX_VENUE_NAME_CHARS: usize = 80;

/// Roughly Vietnam: a Maps link elsewhere is a mistake.
pub fn in_vietnam(lat: f64, lng: f64) -> bool {
    (8.0..=23.5).contains(&lat) && (102.0..=110.0).contains(&lng)
}

/// "Sân Lakeview Thủ Đức" → "san-lakeview-thu-duc".
pub fn slug_base(name: &str) -> String {
    let folded: String = name
        .to_lowercase()
        .chars()
        .map(fold_vietnamese)
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let joined = folded
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let mut base: String = joined.chars().take(30).collect();
    while base.ends_with('-') {
        base.pop();
    }
    if base.len() < 2 {
        "san".to_owned()
    } else {
        base
    }
}

fn fold_vietnamese(c: char) -> char {
    const GROUPS: [(&str, char); 7] = [
        ("àáảãạăằắẳẵặâầấẩẫậ", 'a'),
        ("èéẻẽẹêềếểễệ", 'e'),
        ("ìíỉĩị", 'i'),
        ("òóỏõọôồốổỗộơờớởỡợ", 'o'),
        ("ùúủũụưừứửữự", 'u'),
        ("ỳýỷỹỵ", 'y'),
        ("đ", 'd'),
    ];
    GROUPS
        .iter()
        .find(|(accented, _)| accented.contains(c))
        .map_or(c, |(_, plain)| *plain)
}

/// A unique slug: the name's base and four random hex characters.
pub fn new_slug(name: &str) -> Result<String, RepoError> {
    let bytes = random_bytes::<2>().map_err(|e| RepoError::Unavailable(e.to_string()))?;
    Ok(format!(
        "{}-{:02x}{:02x}",
        slug_base(name),
        bytes[0],
        bytes[1]
    ))
}

/// Port: venues for groups.
pub trait VenueRepository: Send + Sync {
    /// Saves a proposed venue as pending; returns its id.
    fn submit_venue(
        &self,
        venue: &VenueSubmission,
        slug: &str,
    ) -> impl Future<Output = Result<i64, RepoError>> + Send;

    /// Approves or rejects a pending venue; returns its name and who proposed
    /// it, or `None` when no pending venue has that id. Approving fails with
    /// `DecideError::Duplicate` when an approved venue has the same name.
    fn decide_venue(
        &self,
        id: i64,
        status: VenueStatus,
    ) -> impl Future<Output = Result<Option<(String, Option<UserId>)>, DecideError>> + Send;

    fn venues_with_status(
        &self,
        status: VenueStatus,
    ) -> impl Future<Output = Result<Vec<GroupVenue>, RepoError>> + Send;
}

#[derive(Debug, thiserror::Error)]
pub enum DecideError {
    #[error("an approved venue already has this name")]
    Duplicate,
    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Postgres adapter.
#[derive(Clone)]
pub struct PgVenueRepository {
    pool: PgPool,
}

impl PgVenueRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn unavailable(error: sqlx::Error) -> RepoError {
    RepoError::Unavailable(error.to_string())
}

type GroupVenueRow = (i64, String, String, String, Vec<String>, f64, f64);

impl VenueRepository for PgVenueRepository {
    async fn submit_venue(&self, venue: &VenueSubmission, slug: &str) -> Result<i64, RepoError> {
        let sports: Vec<&str> = venue.sports.iter().map(|sport| sport.as_str()).collect();
        sqlx::query_scalar(
            "INSERT INTO venues (slug, name, address, location, active, sports, status,
                 submitted_by, maps_url)
             VALUES ($1, $2, $3, ST_SetSRID(ST_MakePoint($5, $4), 4326)::geography, false,
                 $6, 'pending', $7, $8)
             RETURNING id",
        )
        .bind(slug)
        .bind(&venue.name)
        .bind(&venue.address)
        .bind(venue.lat)
        .bind(venue.lng)
        .bind(&sports)
        .bind(venue.submitted_by.0)
        .bind(&venue.maps_url)
        .fetch_one(&self.pool)
        .await
        .map_err(unavailable)
    }

    async fn decide_venue(
        &self,
        id: i64,
        status: VenueStatus,
    ) -> Result<Option<(String, Option<UserId>)>, DecideError> {
        let row: Option<(String, Option<i64>)> = sqlx::query_as(
            "UPDATE venues SET status = $2 WHERE id = $1 AND status = 'pending'
             RETURNING name, submitted_by",
        )
        .bind(id)
        .bind(status.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| match &error {
            sqlx::Error::Database(db) if db.is_unique_violation() => DecideError::Duplicate,
            _ => DecideError::Repo(unavailable(error)),
        })?;
        Ok(row.map(|(name, by)| (name, by.map(UserId))))
    }

    async fn venues_with_status(&self, status: VenueStatus) -> Result<Vec<GroupVenue>, RepoError> {
        let rows: Vec<GroupVenueRow> = sqlx::query_as(
            "SELECT id, slug, name, address, sports,
                    ST_Y(location::geometry), ST_X(location::geometry)
             FROM venues WHERE status = $1 ORDER BY id",
        )
        .bind(status.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(unavailable)?;
        Ok(rows
            .into_iter()
            .map(|(id, slug, name, address, sports, lat, lng)| GroupVenue {
                id,
                slug,
                name,
                address,
                sports: sports
                    .iter()
                    .filter_map(|sport| Sport::parse(sport))
                    .collect(),
                lat,
                lng,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_fold_vietnamese_and_stay_short() {
        assert_eq!(slug_base("Sân Lakeview Thủ Đức"), "san-lakeview-thu-duc");
        assert_eq!(slug_base("  ĐẠI HỌC!! "), "dai-hoc");
        assert_eq!(slug_base("🏀"), "san");
        assert!(slug_base(&"a".repeat(50)).len() <= 30);
        let slug = new_slug("Sân Lakeview").unwrap();
        assert!(slug.starts_with("san-lakeview-") && slug.len() == "san-lakeview-".len() + 4);
    }

    #[test]
    fn formats_know_their_sport() {
        assert_eq!(GameFormat::parse("3x3").unwrap().sport(), Sport::Basketball);
        assert_eq!(
            GameFormat::parse("seven_a_side").unwrap().sport(),
            Sport::Football
        );
        assert_eq!(GameFormat::parse("futsal"), None);
    }

    #[test]
    fn only_vietnam_is_accepted() {
        assert!(in_vietnam(10.79, 106.75));
        assert!(!in_vietnam(48.85, 2.35));
    }
}
