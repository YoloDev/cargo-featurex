use error_stack::ResultExt;
use lasso::{MiniSpur, Rodeo};
use serde::Deserialize;
use thiserror::Error;

#[derive(Default)]
pub(crate) struct FeaturexMetadata {
	pub(crate) required: Vec<FeatureName>,
	pub(crate) ignored: Vec<FeatureName>,
}

impl FeaturexMetadata {
	pub(crate) fn from_metadata(
		metadata: &serde_json::Value,
		strings: &mut Rodeo<MiniSpur>,
	) -> error_stack::Result<Self, FeaturexMetadataError> {
		let featurex_metadata = metadata.get("featurex");
		FeaturexMetadata::from_featurex_metadata(featurex_metadata, strings)
	}

	fn from_featurex_metadata(
		metadata: Option<&serde_json::Value>,
		strings: &mut Rodeo<MiniSpur>,
	) -> error_stack::Result<Self, FeaturexMetadataError> {
		match metadata {
			None => Ok(Self::default()),
			Some(metadata) => {
				let value: SerdeProxy =
					serde_json::from_value(metadata.clone()).change_context(FeaturexMetadataError)?;

				let required = parse_features(value.required, strings);
				let ignored = parse_features(value.ignored, strings);

				Ok(Self { required, ignored })
			}
		}
	}
}

fn parse_features(features: Vec<String>, strings: &mut Rodeo<MiniSpur>) -> Vec<FeatureName> {
	features
		.into_iter()
		.map(|feature| {
			if feature.ends_with('?') {
				let name = strings.get_or_intern(&feature[..feature.len() - 1]);
				FeatureName::Optional(name)
			} else {
				let name = strings.get_or_intern(&feature);
				FeatureName::Required(name)
			}
		})
		.collect()
}

pub(crate) enum FeatureName {
	Required(MiniSpur),
	Optional(MiniSpur),
}

#[derive(Deserialize)]
struct SerdeProxy {
	#[serde(default)]
	required: Vec<String>,

	#[serde(default)]
	ignored: Vec<String>,
}

#[derive(Debug, Error)]
#[error("failed to parse featurex metadata")]
pub(crate) struct FeaturexMetadataError;
