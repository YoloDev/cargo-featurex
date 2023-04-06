pub(crate) mod collect_result;
pub(crate) mod metadata;

pub mod feature_set;

use cargo_metadata::{MetadataCommand, PackageId};
use collect_result::CollectResult;
use error_stack::{IntoReport, ResultExt};
use feature_set::{FeatureSet, FeatureSetBuilder};
use itertools::Itertools;
use lasso::{MiniSpur, Rodeo, RodeoReader};
use metadata::FeaturexMetadata;
use std::{
	path::{Path, PathBuf},
	rc::Rc,
};
use thiserror::Error;

pub struct Workspace {
	_strings: Rc<RodeoReader<MiniSpur>>,
	packages: Vec<Package>,
	root: Option<usize>,
}

impl Workspace {
	pub fn packages(&self) -> &[Package] {
		match self.root {
			Some(root) => &self.packages[root..=root],
			None => &self.packages,
		}
	}

	pub fn all_packages(&self) -> &[Package] {
		&self.packages
	}

	pub fn root(&self) -> Option<&Package> {
		self.root.map(|i| &self.packages[i])
	}
}

#[derive(Debug)]
pub struct PackageInfo {
	pub name: String,
	pub version: String,
	pub id: PackageId,
}

pub struct Package {
	pub name: String,
	pub version: String,
	pub id: PackageId,
	pub manifest_path: PathBuf,
	pub features: FeatureSet,
}

impl Package {
	pub fn id(&self) -> &str {
		&self.id.repr
	}

	pub fn name(&self) -> &str {
		&self.name
	}

	pub fn version(&self) -> &str {
		&self.version
	}

	pub fn manifest_path(&self) -> &Path {
		&self.manifest_path
	}

	pub fn info(&self) -> PackageInfo {
		PackageInfo {
			name: self.name.clone(),
			version: self.version.clone(),
			id: self.id.clone(),
		}
	}
}

struct PackageBuilder {
	name: String,
	version: String,
	id: PackageId,
	manifest_path: PathBuf,
	features: FeatureSetBuilder,
}

impl PackageBuilder {
	fn build(self, strings: Rc<RodeoReader<MiniSpur>>) -> Package {
		Package {
			name: self.name,
			version: self.version,
			id: self.id,
			manifest_path: self.manifest_path,
			features: self.features.build(strings),
		}
	}
}

impl Package {
	fn builder(
		strings: &mut Rodeo<MiniSpur>,
		package: &cargo_metadata::Package,
		metadata: &FeaturexMetadata,
	) -> error_stack::Result<PackageBuilder, PackageError> {
		let features = FeatureSet::builder(strings, package, metadata)
			.change_context_lazy(|| PackageError::new(package))?;

		Ok(PackageBuilder {
			name: package.name.clone(),
			version: package.version.to_string(),
			id: package.id.clone(),
			manifest_path: package.manifest_path.clone().into(),
			features,
		})
	}
}

#[derive(Debug, Error)]
#[error("failed to get workspace")]
pub struct GetWorkspaceError;

#[derive(Debug, Error)]
#[error("failed to get package {package_id}")]
pub struct PackageError {
	package_id: PackageId,
}

impl PackageError {
	fn new(package: &cargo_metadata::Package) -> Self {
		Self {
			package_id: package.id.clone(),
		}
	}
}

pub fn workspace(
	manifest_path: Option<&Path>,
) -> error_stack::Result<Workspace, GetWorkspaceError> {
	let mut metadata = MetadataCommand::new();
	if let Some(manifest_path) = manifest_path {
		metadata.manifest_path(manifest_path);
	}

	let metadata = metadata
		.exec()
		.into_report()
		.change_context(GetWorkspaceError)?;

	let metadata = Rc::new(metadata);
	let mut strings = Rodeo::new();
	let workspace_metadata =
		FeaturexMetadata::from_metadata(&metadata.workspace_metadata, &mut strings)
			.change_context(GetWorkspaceError)?;

	let packages = metadata
		.packages
		.iter()
		.filter_map(|pkg| {
			metadata
				.workspace_members
				.contains(&pkg.id)
				.then(|| Package::builder(&mut strings, pkg, &workspace_metadata))
		})
		.collect::<CollectResult<Vec<_>, _>>()
		.into_result()
		.change_context(GetWorkspaceError)?;

	let strings = Rc::new(strings.into_reader());
	let packages = packages
		.into_iter()
		.map({
			let strings = Rc::clone(&strings);
			move |pkg| pkg.build(Rc::clone(&strings))
		})
		.collect_vec();

	let root = metadata.root_package().map(|p| {
		packages
			.iter()
			.find_position(|pkg| pkg.id == p.id)
			.unwrap()
			.0
	});

	Ok(Workspace {
		_strings: strings,
		packages,
		root,
	})
}
