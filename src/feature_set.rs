use crate::CollectResult;
use bit_set::BitSet;
use cargo_metadata::Package;
use error_stack::{report, IntoReport, ResultExt};
use itertools::{Itertools, Powerset};
use serde::Deserialize;
use std::{rc::Rc, vec};
use thiserror::Error;

struct FeatureInfo {
	name: Rc<str>,
	index: usize,
	_direct_deps: BitSet,
	transitive_deps: BitSet,
	set: BitSet,
}

pub struct FeatureSet {
	required: BitSet,
	ignored: BitSet,
	features: Vec<FeatureInfo>,
	all: BitSet,
}

fn get_bitset(
	features: &[FeatureInfo],
	names: &[String],
) -> error_stack::Result<BitSet, FeatureSetError> {
	let set = names
		.iter()
		.map(|f| {
			features
				.iter()
				.find(|f2| *f2.name == **f)
				.map(|f| f.index)
				.ok_or_else(|| report!(FeatureReferenceError { name: f.clone() }))
		})
		.collect::<CollectResult<_, _>>()
		.into_result()
		.change_context(FeatureSetError)?
		.into_iter()
		.collect();

	Ok(set)
}

impl FeatureSet {
	pub(crate) fn new(package: &Package) -> error_stack::Result<FeatureSet, FeatureSetError> {
		let mut all = BitSet::new();
		let mut features: Vec<FeatureInfo> = Vec::new();
		let mut remaining = package.features.clone();
		while !remaining.is_empty() {
			let (k, v) = remaining
				.iter()
				.map(|(k, v)| (&**k, &**v))
				.find(|(_, v)| v.iter().all(|d| features.iter().any(|f| *f.name == **d)))
				.ok_or_else(|| report!(FeatureSetError))?;

			let name = Rc::from(k);
			let index = features.len();
			let mut direct_deps = BitSet::new();
			let mut transitive_deps = BitSet::new();
			let mut set = transitive_deps.clone();
			set.insert(index);

			for dep in v {
				if dep.starts_with("dep:") {
					continue;
				}

				let dep = features.iter().find(|f| *f.name == **dep).unwrap();
				direct_deps.insert(dep.index);
				transitive_deps.union_with(&dep.transitive_deps);
				set.union_with(&dep.set);
			}

			all.insert(index);
			features.push(FeatureInfo {
				name: Rc::clone(&name),
				index,
				_direct_deps: direct_deps,
				transitive_deps,
				set,
			});

			remaining.remove(&*name);
		}

		let metadata = package.metadata.get("featurex");
		let metadata = FeaturexMetadata::new(metadata).change_context(FeatureSetError)?;
		let required = get_bitset(&features, &metadata.required)?;
		let ignored = get_bitset(&features, &metadata.ignored)?;

		Ok(Self {
			required,
			ignored,
			features,
			all,
		})
	}

	pub fn get(&self, name: &str) -> Option<Feature> {
		self
			.features
			.iter()
			.find(|f| *f.name == *name)
			.map(|f| Feature(f, self))
	}

	pub fn features(&self) -> FeaturesIter {
		FeaturesIter(self.all.into_iter(), self)
	}

	pub fn required(&self) -> impl Iterator<Item = Feature> + '_ {
		self
			.required
			.iter()
			.map(move |i| Feature(&self.features[i], self))
	}

	pub fn permutations(&self) -> Permutaions {
		let mut variable_set = self.all.clone();
		variable_set.difference_with(&self.required);
		variable_set.difference_with(&self.ignored);

		Permutaions {
			iter: variable_set.iter().collect_vec().into_iter().powerset(),
			features: self,
			seen: Vec::new(),
		}
	}
}

impl<'a> IntoIterator for &'a FeatureSet {
	type Item = Feature<'a>;
	type IntoIter = FeaturesIter<'a>;

	fn into_iter(self) -> Self::IntoIter {
		self.features()
	}
}

pub struct Feature<'a>(&'a FeatureInfo, &'a FeatureSet);

impl<'a> Feature<'a> {
	pub fn name(&self) -> &str {
		&self.0.name
	}

	pub fn enabled_by_default(&self) -> bool {
		// self.1.get("default").contains(self)
		if let Some(default) = self.1.get("default") {
			default.is_superset(self)
		} else {
			true
		}
	}

	pub fn is_superset(&self, other: &Feature) -> bool {
		self.0.set.is_superset(&other.0.set)
	}
}

pub struct Features<'a>(BitSet, &'a FeatureSet);

impl<'a> Features<'a> {
	pub fn iter(&self) -> FeaturesIter {
		FeaturesIter(self.0.iter(), self.1)
	}
}

impl<'a> IntoIterator for &'a Features<'a> {
	type Item = Feature<'a>;
	type IntoIter = FeaturesIter<'a>;

	fn into_iter(self) -> Self::IntoIter {
		self.iter()
	}
}

pub struct FeaturesIter<'a>(bit_set::Iter<'a, u32>, &'a FeatureSet);

impl<'a> Iterator for FeaturesIter<'a> {
	type Item = Feature<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		self.0.next().map(|i| Feature(&self.1.features[i], self.1))
	}
}

pub struct Permutaions<'a> {
	iter: Powerset<vec::IntoIter<usize>>,
	features: &'a FeatureSet,
	seen: Vec<BitSet>,
}

impl<'a> Iterator for Permutaions<'a> {
	type Item = Features<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		for indices in self.iter.by_ref() {
			let mut set: BitSet = BitSet::new();
			set.union_with(&self.features.required);
			for i in indices {
				set.union_with(&self.features.features[i].set);
			}

			if self.seen.contains(&set) {
				continue;
			}

			self.seen.push(set.clone());
			return Some(Features(set, self.features));
		}

		None
	}
}

#[derive(Default, Deserialize)]
struct FeaturexMetadata {
	#[serde(default)]
	required: Vec<String>,

	#[serde(default)]
	ignored: Vec<String>,
}

impl FeaturexMetadata {
	fn new(metadata: Option<&serde_json::Value>) -> error_stack::Result<Self, FeaturexMetadataError> {
		match metadata {
			None => Ok(Self::default()),
			Some(metadata) => {
				let value: Self = serde_json::from_value(metadata.clone())
					.into_report()
					.change_context(FeaturexMetadataError)?;

				Ok(value)
			}
		}
	}
}

#[derive(Debug, Error)]
#[error("failed to parse featurex metadata")]
struct FeaturexMetadataError;

#[derive(Debug, Error)]
#[error("failed to produce feature set")]
pub struct FeatureSetError;

#[derive(Debug, Error)]
#[error("feature '{name}' not found")]
pub struct FeatureReferenceError {
	name: String,
}
