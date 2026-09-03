pub fn typo_transform(input: &str) -> String {
    let typos: Vec<char> = input.chars().map(apply_adjacent_swap).collect();
    typos.into_iter().collect()
}

fn apply_adjacent_swap(c: char) -> char {
    match c.to_ascii_lowercase() {
        'a' => maybe_swap(c, 's'),
        's' => maybe_swap(c, 'a'),
        'd' => maybe_swap(c, 'f'),
        'f' => maybe_swap(c, 'd'),
        'j' => maybe_swap(c, 'k'),
        'k' => maybe_swap(c, 'j'),
        'l' => maybe_swap(c, ';'),
        'e' => maybe_swap(c, 'w'),
        'w' => maybe_swap(c, 'e'),
        _ => c,
    }
}

fn maybe_swap(original: char, adjacent: char) -> char {
    if rand_50_percent() {
        if original.is_uppercase() {
            adjacent.to_ascii_uppercase()
        } else {
            adjacent
        }
    } else {
        original
    }
}

fn rand_50_percent() -> bool {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let s = RandomState::new();
    let mut hasher = s.build_hasher();
    hasher.write_u64(1);
    hasher.finish().is_multiple_of(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typo_produces_output() {
        let result = typo_transform("hello world");
        assert_eq!(result.len(), "hello world".len());
    }

    #[test]
    fn test_non_alpha_unchanged() {
        let result = typo_transform("123!@#");
        assert_eq!(result, "123!@#");
    }
}
