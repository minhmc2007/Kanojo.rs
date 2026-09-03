use rand::seq::SliceRandom;
use rand::Rng;

pub struct TypoEngine;

impl TypoEngine {
    pub const fn get_adjacent_keys(c: char) -> &'static [char] {
        match c {
            'q' | 'Q' => &['w', 'a', 's'],
            'w' | 'W' => &['q', 'e', 'a', 's', 'd'],
            'e' | 'E' => &['w', 'r', 's', 'd', 'f'],
            'r' | 'R' => &['e', 't', 'd', 'f', 'g'],
            't' | 'T' => &['r', 'y', 'f', 'g', 'h'],
            'y' | 'Y' => &['t', 'u', 'g', 'h', 'j'],
            'u' | 'U' => &['y', 'i', 'h', 'j', 'k'],
            'i' | 'I' => &['u', 'o', 'j', 'k', 'l'],
            'o' | 'O' => &['i', 'p', 'k', 'l'],
            'p' | 'P' => &['o', 'l'],
            'a' | 'A' => &['q', 'w', 's', 'z'],
            's' | 'S' => &['a', 'd', 'w', 'e', 'z', 'x'],
            'd' | 'D' => &['s', 'f', 'e', 'r', 'x', 'c'],
            'f' | 'F' => &['d', 'g', 'r', 't', 'c', 'v'],
            'g' | 'G' => &['f', 'h', 't', 'y', 'v', 'b'],
            'h' | 'H' => &['g', 'j', 'y', 'u', 'b', 'n'],
            'j' | 'J' => &['h', 'k', 'u', 'i', 'n', 'm'],
            'k' | 'K' => &['j', 'l', 'i', 'o', 'm'],
            'l' | 'L' => &['k', 'o', 'p', 'm'],
            'z' | 'Z' => &['a', 's', 'x'],
            'x' | 'X' => &['z', 'c', 's', 'd'],
            'c' | 'C' => &['x', 'v', 'd', 'f'],
            'v' | 'V' => &['c', 'b', 'f', 'g'],
            'b' | 'B' => &['v', 'n', 'g', 'h'],
            'n' | 'N' => &['b', 'm', 'h', 'j'],
            'm' | 'M' => &['n', 'j', 'k', 'l'],
            _ => &[],
        }
    }

    pub fn inject_typos(text: &str, typo_rate: f64) -> String {
        let mut rng = rand::thread_rng();
        let mut result = String::with_capacity(text.len() + 8);
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let current = chars[i];

            if rng.gen_bool(typo_rate.clamp(0.0, 1.0)) {
                let typo_mode: u8 = rng.gen_range(0..10);

                match typo_mode {
                    0..=6 => {
                        let neighbors = Self::get_adjacent_keys(current);
                        if !neighbors.is_empty() {
                            let mut chosen = *neighbors.choose(&mut rng).unwrap();
                            if current.is_uppercase() {
                                chosen = chosen.to_ascii_uppercase();
                            }
                            result.push(chosen);
                        } else {
                            result.push(current);
                        }
                    }
                    7..=8 => {
                        if i + 1 < chars.len() && chars[i + 1].is_alphabetic() {
                            result.push(chars[i + 1]);
                            result.push(current);
                            i += 2;
                            continue;
                        } else {
                            result.push(current);
                        }
                    }
                    _ => {}
                }
            } else {
                result.push(current);
            }
            i += 1;
        }

        result
    }

    pub fn rate_for_activity(activity: &str) -> f64 {
        match activity {
            "InShower" | "Bath" => 0.12,
            "Rushing" | "Angry" => 0.08,
            "Sleepy" | "WakingUp" => 0.06,
            "Gaming" => 0.04,
            "Idle" | "Studying" => 0.01,
            _ => 0.02,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adjacent_keys_not_empty() {
        let keys = TypoEngine::get_adjacent_keys('a');
        assert!(!keys.is_empty());
    }

    #[test]
    fn test_adjacent_keys_empty_for_unknown() {
        let keys = TypoEngine::get_adjacent_keys('1');
        assert!(keys.is_empty());
    }

    #[test]
    fn test_inject_typos_preserves_length_approx() {
        let result = TypoEngine::inject_typos("hello world", 0.5);
        assert!(result.len() >= 8);
    }

    #[test]
    fn test_rate_for_activity() {
        assert_eq!(TypoEngine::rate_for_activity("InShower"), 0.12);
        assert_eq!(TypoEngine::rate_for_activity("Idle"), 0.01);
        assert_eq!(TypoEngine::rate_for_activity("Unknown"), 0.02);
    }

    #[test]
    fn test_zero_rate_no_change() {
        let result = TypoEngine::inject_typos("hello", 0.0);
        assert_eq!(result, "hello");
    }
}
