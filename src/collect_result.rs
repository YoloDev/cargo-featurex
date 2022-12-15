pub(crate) enum CollectResult<T, E> {
	Ok(T),
	Err(error_stack::Report<E>),
}

impl<T, E> CollectResult<T, E> {
	pub(crate) fn into_result(self) -> error_stack::Result<T, E> {
		match self {
			Self::Ok(results) => Ok(results),
			Self::Err(report) => Err(report),
		}
	}
}

impl<A, E, V> FromIterator<error_stack::Result<A, E>> for CollectResult<V, E>
where
	V: FromIterator<A>,
{
	fn from_iter<I: IntoIterator<Item = error_stack::Result<A, E>>>(iter: I) -> Self {
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

		Self::Ok(vec.into_iter().collect())
	}
}
