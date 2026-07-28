use crate::check::expr::AggregateKind;

pub(in crate::check) fn aggregate(
	kind: AggregateKind,
	values: Vec<f64>,
	percentile: Option<f64>,
) -> Option<f64> {
	match kind {
		AggregateKind::Sum => Some(values.iter().sum()),
		AggregateKind::Max => values.into_iter().reduce(f64::max),
		AggregateKind::Min => values.into_iter().reduce(f64::min),
		AggregateKind::Avg => average(&values),
		AggregateKind::Median => percentile_value(values, 50.0),
		AggregateKind::Percentile => percentile_value(values, percentile?),
		AggregateKind::Stddev => variance(&values).map(f64::sqrt),
		AggregateKind::Var => variance(&values),
		AggregateKind::Cv => {
			let mean = average(&values)?;
			if mean == 0.0 {
				return None;
			}
			Some(variance(&values)?.sqrt() / mean.abs())
		}
		AggregateKind::Gini => gini(&values),
	}
}

fn average(values: &[f64]) -> Option<f64> {
	(!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn variance(values: &[f64]) -> Option<f64> {
	let mean = average(values)?;
	Some(
		values
			.iter()
			.map(|value| (value - mean).powi(2))
			.sum::<f64>()
			/ values.len() as f64,
	)
}

fn percentile_value(mut values: Vec<f64>, percentile: f64) -> Option<f64> {
	if values.is_empty() || !(0.0..=100.0).contains(&percentile) {
		return None;
	}
	values.sort_by(|left, right| left.total_cmp(right));
	let rank = (percentile / 100.0) * (values.len().saturating_sub(1)) as f64;
	let lower = rank.floor() as usize;
	let upper = rank.ceil() as usize;
	if lower == upper {
		return values.get(lower).copied();
	}
	let weight = rank - lower as f64;
	Some(values[lower] + (values[upper] - values[lower]) * weight)
}

fn gini(values: &[f64]) -> Option<f64> {
	if values.is_empty() {
		return None;
	}
	let mut sorted = values
		.iter()
		.copied()
		.filter(|value| *value >= 0.0)
		.collect::<Vec<_>>();
	if sorted.len() != values.len() {
		return None;
	}
	sorted.sort_by(|left, right| left.total_cmp(right));
	let sum = sorted.iter().sum::<f64>();
	if sum == 0.0 {
		return Some(0.0);
	}
	let weighted = sorted
		.iter()
		.enumerate()
		.map(|(index, value)| (index as f64 + 1.0) * value)
		.sum::<f64>();
	Some(
		(2.0 * weighted) / (sorted.len() as f64 * sum)
			- (sorted.len() as f64 + 1.0) / sorted.len() as f64,
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn descriptive_statistics_share_one_deterministic_implementation() {
		let values = vec![1.0, 1.0, 10.0];
		assert_eq!(
			aggregate(AggregateKind::Avg, values.clone(), None),
			Some(4.0)
		);
		assert_eq!(
			aggregate(AggregateKind::Median, values.clone(), None),
			Some(1.0)
		);
		assert_eq!(
			aggregate(AggregateKind::Percentile, values.clone(), Some(90.0)),
			Some(8.2)
		);
		assert_eq!(aggregate(AggregateKind::Gini, values, None), Some(0.5));
	}
}
