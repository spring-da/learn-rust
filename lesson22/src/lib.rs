use std::collections::HashSet;

pub fn count_linear(allowed: &[i32], queries: &[i32]) -> usize {
    queries.iter().filter(|query| allowed.contains(query)).count()
}

pub fn build_index(allowed: &[i32]) -> HashSet<i32> {
    allowed.iter().copied().collect()
}

pub fn count_indexed(index: &HashSet<i32>, queries: &[i32]) -> usize {
    queries.iter().filter(|query| index.contains(query)).count()
}

pub fn sample_data() -> (Vec<i32>, Vec<i32>) {
    ((0..4096).map(|id| id * 2).collect(), (0..10000).map(|id| id % 8192).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_semantics_preserve_query_multiplicity() {
        let cases = [
            (vec![], vec![], 0), (vec![], vec![1], 0),
            (vec![1, 1, 2], vec![1, 1, 3], 2),
            (vec![-2, 0, 7], vec![0, -2, -2, 8], 3),
        ];
        for (allowed, queries, expected) in cases {
            assert_eq!(count_linear(&allowed, &queries), expected);
            assert_eq!(count_indexed(&build_index(&allowed), &queries), expected);
        }
    }

    #[test]
    fn index_owns_independent_keys() {
        let mut allowed = vec![1, 2];
        let index = build_index(&allowed);
        allowed[0] = 9;
        assert!(index.contains(&1));
        assert!(!index.contains(&9));
    }

    #[test]
    fn deterministic_generated_cases_match_reference() {
        let mut state = 12345_u64;
        let mut next = || { state = state.wrapping_mul(6364136223846793005).wrapping_add(1); (state >> 32) as i32 };
        for case in 0..1000 {
            let allowed: Vec<_> = (0..case % 65).map(|_| next() % 32).collect();
            let queries: Vec<_> = (0..case % 73).map(|_| next() % 32).collect();
            assert_eq!(count_indexed(&build_index(&allowed), &queries), count_linear(&allowed, &queries));
        }
    }
}
