use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub nsfw: Option<bool>,
    #[serde(default)]
    pub nsfw_level: Option<u32>,
    #[serde(default)]
    pub availability: Option<String>,
    #[serde(default)]
    pub supports_generation: Option<bool>,
    #[serde(default)]
    pub stats: Option<Stats>,
    #[serde(default)]
    pub creator: Option<Creator>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub model_versions: Vec<ModelVersion>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Creator {
    pub username: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    #[serde(default)]
    pub download_count: Option<u64>,
    #[serde(default)]
    pub thumbs_up_count: Option<u64>,
    #[serde(default)]
    pub thumbs_down_count: Option<u64>,
    #[serde(default)]
    pub comment_count: Option<u64>,
    #[serde(default)]
    pub tipped_amount_count: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVersion {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub base_model: Option<String>,
    #[serde(default)]
    pub base_model_type: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub availability: Option<String>,
    #[serde(default)]
    pub supports_generation: Option<bool>,
    #[serde(default)]
    pub stats: Option<Stats>,
    #[serde(default)]
    pub files: Vec<ModelFile>,
    #[serde(default)]
    pub images: Vec<PreviewImage>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFile {
    pub id: u64,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub size_kb: Option<f64>,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub primary: Option<bool>,
    #[serde(default)]
    pub hashes: BTreeMap<String, String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewImage {
    #[serde(default)]
    pub id: Option<u64>,
    pub url: String,
    #[serde(default)]
    pub nsfw_level: Option<u32>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
