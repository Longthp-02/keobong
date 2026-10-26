//! Reading a venue's place from a Google Maps link an organiser pasted
//! ("https://maps.app.goo.gl/…"). Short links are followed hop by hop, only on
//! Google hosts, so the server never fetches an address a user chose freely.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// What a Maps link says about a place.
#[derive(Debug, Clone, PartialEq)]
pub struct MapsPlace {
    pub lat: f64,
    pub lng: f64,
    /// The place's name in the link ("/place/<name>/"), when it has one.
    pub label: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum MapsError {
    #[error("not a Google Maps link")]
    NotMaps,
    #[error("the link has no coordinates")]
    NoPlace,
    #[error("could not open the link: {0}")]
    Failed(String),
}

pub type MapsFuture<'a> = Pin<Box<dyn Future<Output = Result<MapsPlace, MapsError>> + Send + 'a>>;

/// Port: where a Maps link points.
pub trait MapsLocator: Send + Sync {
    fn locate<'a>(&'a self, url: &'a str) -> MapsFuture<'a>;
}

/// Hosts a Maps link may be on or redirect through.
const GOOGLE_HOSTS: [&str; 6] = [
    "maps.app.goo.gl",
    "goo.gl",
    "maps.google.com",
    "www.google.com",
    "google.com",
    "www.google.com.vn",
];

const MAX_HOPS: usize = 5;

/// The first Maps link in a message.
pub fn find_maps_link(text: &str) -> Option<&str> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| matches!(c, '<' | '>' | '(' | ')' | ',')))
        .find(|word| google_host(word).is_some())
}

/// The URL's host when it is an https Google Maps host.
fn google_host(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    GOOGLE_HOSTS.contains(&host).then_some(host)
}

/// Reads the place from a long Maps URL. A place pin (`!3d<lat>!4d<lng>`) wins
/// over the map centre (`@<lat>,<lng>`) and a `q=`/`query=`/`ll=` point.
pub fn place_from_url(url: &str) -> Option<MapsPlace> {
    let number = |text: &str| -> Option<(f64, usize)> {
        let end = text
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_digit() || matches!(c, '.' | '-')))
            .map_or(text.len(), |(index, _)| index);
        text[..end].parse().ok().map(|value| (value, end))
    };
    let pair_after = |marker: &str, separator: &str| -> Option<(f64, f64)> {
        let start = url.find(marker)? + marker.len();
        let (lat, used) = number(&url[start..])?;
        let rest = url[start + used..].strip_prefix(separator)?;
        let (lng, _) = number(rest)?;
        Some((lat, lng))
    };
    let point = pair_after("!3d", "!4d")
        .or_else(|| pair_after("@", ","))
        .or_else(|| {
            ["q=", "query=", "ll="]
                .iter()
                .find_map(|marker| pair_after(marker, ","))
        })
        .or_else(|| {
            ["q=", "query="]
                .iter()
                .find_map(|marker| pair_after(marker, "%2C"))
        })?;
    let label = url.split("/place/").nth(1).and_then(|rest| {
        let raw = rest.split(['/', '@', '?']).next()?;
        let decoded = percent_decode(&raw.replace('+', " "));
        let trimmed = decoded.trim().to_owned();
        (!trimmed.is_empty()).then_some(trimmed)
    });
    Some(MapsPlace {
        lat: point.0,
        lng: point.1,
        label,
    })
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        // Bytes, not str slices: a '%' may sit right before a multibyte character.
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        if bytes[index] == b'%'
            && let Some(byte) = hex
        {
            out.push(byte);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Follows a Maps link's redirects with HTTP and reads the final URL.
pub struct HttpMapsLocator {
    http: reqwest::Client,
}

impl HttpMapsLocator {
    pub fn new() -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self { http })
    }
}

impl MapsLocator for HttpMapsLocator {
    fn locate<'a>(&'a self, url: &'a str) -> MapsFuture<'a> {
        Box::pin(async move {
            let mut current = url.to_owned();
            for _ in 0..=MAX_HOPS {
                google_host(&current).ok_or(MapsError::NotMaps)?;
                if let Some(place) = place_from_url(&current) {
                    return Ok(place);
                }
                let response = self
                    .http
                    .get(&current)
                    .send()
                    .await
                    .map_err(|error| MapsError::Failed(error.without_url().to_string()))?;
                let Some(next) = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                else {
                    return Err(MapsError::NoPlace);
                };
                // A relative redirect stays on the same Google host.
                current = if next.starts_with('/') {
                    let host = google_host(&current).ok_or(MapsError::NotMaps)?;
                    format!("https://{host}{next}")
                } else {
                    next.to_owned()
                };
            }
            Err(MapsError::NoPlace)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_maps_link_in_a_message() {
        assert_eq!(
            find_maps_link("thêm sân Lakeview https://maps.app.goo.gl/AbC123 bóng rổ"),
            Some("https://maps.app.goo.gl/AbC123")
        );
        assert_eq!(
            find_maps_link("thêm sân Lakeview http://evil.example/x"),
            None
        );
        assert_eq!(find_maps_link("https://maps.app.goo.gl.evil.com/x"), None);
        assert_eq!(find_maps_link("http://maps.app.goo.gl/x"), None);
    }

    #[test]
    fn reads_the_place_pin_and_name_from_a_long_link() {
        let url = "https://www.google.com/maps/place/S%C3%A2n+b%C3%B3ng+r%E1%BB%95+Lakeview/@10.7901,106.7500,17z/data=!3m1!4b1!4m6!3m5!1s0x0:0x1!8m2!3d10.7905123!4d106.7512345";
        let place = place_from_url(url).unwrap();
        assert_eq!((place.lat, place.lng), (10.7905123, 106.7512345));
        assert_eq!(place.label.as_deref(), Some("Sân bóng rổ Lakeview"));
        // A stray '%' before a multibyte character does not panic.
        assert_eq!(percent_decode("a%é"), "a%é");
    }

    #[test]
    fn falls_back_to_the_map_centre_or_a_query_point() {
        let centre = place_from_url("https://www.google.com/maps/@10.8,106.7,15z").unwrap();
        assert_eq!((centre.lat, centre.lng), (10.8, 106.7));
        assert_eq!(centre.label, None);
        let query = place_from_url("https://maps.google.com/?q=10.81,-106.72").unwrap();
        assert_eq!((query.lat, query.lng), (10.81, -106.72));
        assert_eq!(
            place_from_url("https://maps.app.goo.gl/AbC123"),
            None,
            "a short link must be followed first"
        );
    }
}
