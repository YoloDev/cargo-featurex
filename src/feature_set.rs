use crate::{
	CollectResult,
	metadata::{FeatureName, FeaturexMetadata},
};
use cargo_metadata::Package;
use error_stack::{ResultExt, report};
use itertools::{Itertools, Powerset};
use lasso::{MiniSpur, Rodeo, RodeoReader};
use std::{fmt, rc::Rc, vec};
use thiserror::Error;

type BitStorage = u64;
type BitSet = bitarr::BitSet<BitStorage>;

#[derive(Clone)]
struct FeatureInfo {
	name: MiniSpur,
	index: u32,
	_direct_deps: BitSet,
	transitive_deps: BitSet,
	set: BitSet,
}

#[derive(Clone)]
pub struct FeatureSet {
	required: BitSet,
	ignored: BitSet,
	default: BitSet,
	features: Vec<FeatureInfo>,
	all: BitSet,
	strings: Rc<RodeoReader<MiniSpur>>,
}

impl fmt::Debug for FeatureSet {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		struct DebugFeatures<'a>(&'a [FeatureInfo], &'a RodeoReader<MiniSpur>);

		impl<'a> fmt::Debug for DebugFeatures<'a> {
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				f.debug_list()
					.entries(self.0.iter().map(|f| self.1.resolve(&f.name)))
					.finish()
			}
		}

		f.debug_struct("FeatureSet")
			.field("features", &DebugFeatures(&self.features, &self.strings))
			.finish_non_exhaustive()
	}
}

pub struct FeatureSetBuilder {
	required: BitSet,
	ignored: BitSet,
	default: BitSet,
	features: Vec<FeatureInfo>,
	all: BitSet,
}

impl FeatureSetBuilder {
	pub fn build(self, strings: Rc<RodeoReader<MiniSpur>>) -> FeatureSet {
		FeatureSet {
			required: self.required,
			ignored: self.ignored,
			default: self.default,
			features: self.features,
			all: self.all,
			strings,
		}
	}
}

fn find_feature(
	features: &[FeatureInfo],
	name: &FeatureName,
	strings: &Rodeo<MiniSpur>,
) -> Option<error_stack::Result<u32, FeatureReferenceError>> {
	match name {
		FeatureName::Optional(n) => features.iter().find(|f| f.name == *n).map(|f| Ok(f.index)),
		FeatureName::Required(n) => Some(
			features
				.iter()
				.find(|f| f.name == *n)
				.map(|f| f.index)
				.ok_or_else(|| {
					report!(FeatureReferenceError {
						name: strings.resolve(n).to_owned(),
					})
				}),
		),
	}
}

fn get_bitset(
	features: &[FeatureInfo],
	package_names: &[FeatureName],
	workspace_names: &[FeatureName],
	strings: &Rodeo<MiniSpur>,
) -> error_stack::Result<BitSet, CreateMetadataBitSetError> {
	let from_package: Result<BitSet, _> = package_names
		.iter()
		.filter_map(|n| find_feature(features, n, strings))
		.collect::<CollectResult<_, _>>()
		.into_result()
		.change_context(CreateMetadataBitSetError::Package);

	let from_workspace: Result<BitSet, _> = workspace_names
		.iter()
		.filter_map(|n| find_feature(features, n, strings))
		.collect::<CollectResult<_, _>>()
		.into_result()
		.change_context(CreateMetadataBitSetError::Workspace);

	match (from_package, from_workspace) {
		(Ok(p), Ok(w)) => Ok(p.union(&w)),
		(Err(e), Ok(_)) => Err(e),
		(Ok(_), Err(e)) => Err(e),
		(Err(mut e1), Err(e2)) => {
			e1.extend_one(e2);
			Err(e1)
		}
	}
}

impl FeatureSet {
	pub(crate) fn builder(
		strings: &mut Rodeo<MiniSpur>,
		package: &Package,
		workspace_metadata: &FeaturexMetadata,
	) -> error_stack::Result<FeatureSetBuilder, FeatureSetError> {
		let mut all = BitSet::new();
		let mut features: Vec<FeatureInfo> = Vec::new();
		let feature_names = package
			.features
			.iter()
			.map(|(k, v)| (strings.get_or_intern(&**k), v))
			.collect_vec();
		let names = feature_names.iter().map(|(k, _)| *k).collect_vec();

		let mut remaining = feature_names
			.into_iter()
			.map(|(k, v)| {
				(
					k,
					v.iter()
						.filter_map(|s| {
							strings.get(&**s).and_then(|s| match names.contains(&s) {
								true => Some(s),
								false => None,
							})
						})
						.collect_vec(),
				)
			})
			.collect_vec();
		while !remaining.is_empty() {
			// find feature where all dependencies are already in the set
			let (idx, _) = remaining
				.iter()
				.find_position(|(_, v)| v.iter().all(|d| features.iter().any(|f| f.name == *d)))
				.ok_or_else(|| report!(FeatureSetError))?;

			let (name, v) = remaining.swap_remove(idx);
			let index = features.len() as u32;
			let mut direct_deps = BitSet::new();
			let mut transitive_deps = BitSet::new();
			let mut set = transitive_deps;
			set.set(index);

			for dep in v {
				let dep = features.iter().find(|f| f.name == dep).unwrap();
				direct_deps.set(dep.index);
				transitive_deps.union_with(&dep.transitive_deps);
				set.union_with(&dep.set);
			}

			all.set(index);
			features.push(FeatureInfo {
				name,
				index,
				_direct_deps: direct_deps,
				transitive_deps,
				set,
			});
		}

		let metadata = FeaturexMetadata::from_metadata(&package.metadata, strings)
			.change_context(FeatureSetError)?;
		let required = get_bitset(
			&features,
			&metadata.required,
			&workspace_metadata.required,
			strings,
		)
		.change_context_lazy(|| MetadataBitSetError {
			name: "required".into(),
		})
		.change_context(FeatureSetError)?;
		let ignored = get_bitset(
			&features,
			&metadata.ignored,
			&workspace_metadata.ignored,
			strings,
		)
		.change_context_lazy(|| MetadataBitSetError {
			name: "ignored".into(),
		})
		.change_context(FeatureSetError)?;

		let default = match strings.get("default") {
			None => BitSet::new(),
			Some(default) => features
				.iter()
				.find(|f| f.name == default)
				.map(|f| f.set)
				.unwrap_or_default(),
		};

		Ok(FeatureSetBuilder {
			required,
			ignored,
			features,
			default,
			all,
		})
	}

	pub fn get(&self, name: &str) -> Option<Feature> {
		self
			.strings
			.get(name)
			.and_then(|name| self.features.iter().find(|f| f.name == name))
			.map(|f| Feature::new(f, self))
	}

	pub fn features(&self) -> FeaturesIter {
		FeaturesIter(self.all.iter().enumerate(), self)
	}

	pub fn required(&self) -> impl Iterator<Item = Feature> + '_ {
		self
			.required
			.iter()
			.enumerate()
			.filter(|(_, b)| *b)
			.map(|(i, _)| Feature::new(&self.features[i], self))
	}

	pub fn permutations(&self) -> Permutations {
		let mut variable_set = self.all;
		variable_set.difference_with(&self.required);
		variable_set.difference_with(&self.ignored);

		Permutations {
			iter: variable_set
				.iter()
				.enumerate()
				.filter_map(|(i, b)| b.then_some(i))
				.collect_vec()
				.into_iter()
				.powerset(),
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

pub struct Feature<'a> {
	feature: &'a FeatureInfo,
	set: &'a FeatureSet,
}

impl<'a> Feature<'a> {
	fn new(feature: &'a FeatureInfo, set: &'a FeatureSet) -> Self {
		Self { feature, set }
	}

	pub fn name(&self) -> &str {
		self.set.strings.resolve(&self.feature.name)
	}

	pub fn enabled_by_default(&self) -> bool {
		self.set.default.is_superset(&self.feature.set)
	}

	pub fn is_superset(&self, other: &Feature) -> bool {
		self.feature.set.is_superset(&other.feature.set)
	}
}

pub struct Features<'a>(BitSet, &'a FeatureSet);

impl<'a> Features<'a> {
	pub fn iter(&self) -> FeaturesIter {
		FeaturesIter(self.0.iter().enumerate(), self.1)
	}
}

impl<'a> IntoIterator for &'a Features<'a> {
	type Item = Feature<'a>;
	type IntoIter = FeaturesIter<'a>;

	fn into_iter(self) -> Self::IntoIter {
		self.iter()
	}
}

pub struct FeaturesIter<'a>(
	std::iter::Enumerate<bitarr::iter::Bits<&'a BitStorage>>,
	&'a FeatureSet,
);

impl<'a> Iterator for FeaturesIter<'a> {
	type Item = Feature<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		for (i, b) in &mut self.0 {
			if b {
				return Some(Feature::new(&self.1.features[i], self.1));
			}
		}

		None
	}
}

pub struct Permutations<'a> {
	iter: Powerset<vec::IntoIter<usize>>,
	features: &'a FeatureSet,
	seen: Vec<BitSet>,
}

impl<'a> Iterator for Permutations<'a> {
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

			self.seen.push(set);
			return Some(Features(set, self.features));
		}

		None
	}
}

#[derive(Debug, Error)]
#[error("failed to produce feature set")]
pub struct FeatureSetError;

#[derive(Debug, Error)]
#[error("feature '{name}' not found")]
pub struct FeatureReferenceError {
	name: String,
}

#[derive(Debug, Error)]
enum CreateMetadataBitSetError {
	#[error("failed to produce feature set defined in package metadata")]
	Package,
	#[error("failed to produce feature set defined in workspace metadata")]
	Workspace,
}

#[derive(Debug, Error)]
#[error("failed to produce bitset for metadata '{name}'")]
pub struct MetadataBitSetError {
	name: String,
}
