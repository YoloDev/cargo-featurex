pub mod feature_set;

use cargo_metadata::{Metadata, MetadataCommand, PackageId};
use error_stack::{IntoReport, ResultExt};
use feature_set::FeatureSet;
use std::{
	path::{Path, PathBuf},
	rc::Rc,
};
use thiserror::Error;

pub struct Package {
	pub name: String,
	pub id: PackageId,
	pub manifest_path: PathBuf,
	pub features: FeatureSet,
}

impl Package {
	fn new(
		package: &cargo_metadata::Package,
		_metadata: Rc<Metadata>,
	) -> error_stack::Result<Self, PackageError> {
		let features = FeatureSet::new(package).change_context_lazy(|| PackageError::new(package))?;

		Ok(Self {
			name: package.name.clone(),
			id: package.id.clone(),
			manifest_path: package.manifest_path.clone().into(),
			features,
		})
	}

	pub fn manifest_path(&self) -> &Path {
		&self.manifest_path
	}
}

#[derive(Debug, Error)]
#[error("failed to get packages")]
pub struct GetPackagesError;

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

enum CollectResult<T, E> {
	Ok(Vec<T>),
	Err(error_stack::Report<E>),
}

impl<T, E> CollectResult<T, E> {
	fn into_result(self) -> error_stack::Result<Vec<T>, E> {
		match self {
			Self::Ok(vec) => Ok(vec),
			Self::Err(report) => Err(report),
		}
	}
}

impl<T, E> FromIterator<error_stack::Result<T, E>> for CollectResult<T, E> {
	fn from_iter<I: IntoIterator<Item = error_stack::Result<T, E>>>(iter: I) -> Self {
		let mut iter = iter.into_iter();
		let mut vec = Vec::with_capacity(iter.size_hint().0);

		while let Some(item) = iter.next() {
			match item {
				Ok(item) => vec.push(item),
				Err(mut report) => {
					report.extend(iter.filter_map(Result::err));

					return Self::Err(report);
				}
			}
		}

		Self::Ok(vec)
	}
}

pub fn packages(
	manifest_path: Option<&Path>,
) -> error_stack::Result<Vec<Package>, GetPackagesError> {
	let mut metadata = MetadataCommand::new();
	if let Some(manifest_path) = manifest_path {
		metadata.manifest_path(manifest_path);
	}

	let metadata = metadata
		.exec()
		.into_report()
		.change_context(GetPackagesError)?;

	let metadata = Rc::new(metadata);

	let packages = metadata
		.packages
		.iter()
		.filter({
			let metadata = metadata.clone();
			move |pkg| metadata.workspace_members.contains(&pkg.id)
		})
		.map({
			let metadata = metadata.clone();
			move |pkg| Package::new(pkg, metadata.clone())
		})
		.collect::<CollectResult<_, _>>()
		.into_result()
		.change_context(GetPackagesError)?;

	Ok(packages)
}
