use std::mem::swap;

pub fn did_you_mean(input: &str, candidates: impl IntoIterator<Item = String>) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    let mut candidates = candidates
        .into_iter()
        .filter(|candidate| !candidate.trim().is_empty())
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();

    if let Some(prefix_match) = candidates
        .iter()
        .find(|candidate| candidate.starts_with(input) || input.starts_with(candidate.as_str()))
    {
        return Some(prefix_match.clone());
    }

    candidates
        .into_iter()
        .filter_map(|candidate| {
            let distance = edit_distance(input, &candidate);
            (distance <= suggestion_threshold(input, &candidate)).then_some((
                distance,
                common_prefix_len(input, &candidate),
                candidate,
            ))
        })
        .min_by(
            |(left_distance, left_prefix_len, left), (right_distance, right_prefix_len, right)| {
                left_distance
                    .cmp(right_distance)
                    .then_with(|| right_prefix_len.cmp(left_prefix_len))
                    .then_with(|| left.cmp(right))
            },
        )
        .map(|(_, _, candidate)| candidate)
}

pub fn did_you_mean_message(
    input: &str,
    candidates: impl IntoIterator<Item = String>,
) -> Option<String> {
    did_you_mean(input, candidates).map(|candidate| format!("Did you mean '{candidate}'?"))
}

fn suggestion_threshold(input: &str, candidate: &str) -> usize {
    let max_len = input.chars().count().max(candidate.chars().count());
    if max_len <= 4 {
        1
    } else {
        2
    }
}

fn edit_distance(left: &str, right: &str) -> usize {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];

    for (left_index, left_ch) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_ch) in right.iter().enumerate() {
            let insertion = current[right_index] + 1;
            let deletion = previous[right_index + 1] + 1;
            let substitution = previous[right_index] + usize::from(left_ch != right_ch);
            current[right_index + 1] = insertion.min(deletion).min(substitution);
        }
        swap(&mut previous, &mut current);
    }

    previous[right.len()]
}

fn common_prefix_len(left: &str, right: &str) -> usize {
    left.chars()
        .zip(right.chars())
        .take_while(|(left_ch, right_ch)| left_ch == right_ch)
        .count()
}

#[cfg(test)]
mod tests {
    use super::did_you_mean;

    #[test]
    fn suggestion_prefers_prefix_match() {
        assert_eq!(
            did_you_mean(
                "obj",
                ["collection", "object", "object-relation"].map(str::to_string)
            )
            .as_deref(),
            Some("object")
        );
    }

    #[test]
    fn suggestion_accepts_near_typo() {
        assert_eq!(
            did_you_mean("clas", ["class", "config"].map(str::to_string)).as_deref(),
            Some("class")
        );
    }

    #[test]
    fn suggestion_prefers_shared_prefix_on_distance_tie() {
        assert_eq!(
            did_you_mean("dsc", ["asc", "desc"].map(str::to_string)).as_deref(),
            Some("desc")
        );
    }

    #[test]
    fn suggestion_rejects_distant_match() {
        assert!(did_you_mean("zzzz", ["class", "object"].map(str::to_string)).is_none());
    }
}
