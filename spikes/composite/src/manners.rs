//! A dry word for the exit statuses everyone meets. It speaks only when the
//! pointer rests on a block's lamp, and only for a few numbers: the ones that
//! have a meaning beginners are usually left to guess, and one that has a joke.
//! Plain and accurate first; the joke, once.

/// What an exit status means, in a short clause, or None when there is nothing useful to add.
pub fn exit_words(code: i32) -> Option<&'static str> {
    Some(match code {
        1 => "a general failure: the command said no without saying why",
        2 => "misused: wrong arguments, or a shell built-in used wrongly",
        42 => "the answer, though not necessarily to the question you asked",
        126 => "found, but could not be run: check it is executable",
        127 => "not found: check the spelling, and that it is on your PATH",
        130 => "interrupted: you pressed Ctrl+C",
        137 => "killed (SIGKILL): often the system running out of memory",
        139 => "a segmentation fault: the program touched memory it should not",
        143 => "terminated (SIGTERM): something asked it to stop, and it did",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_few_numbers_have_words_and_the_rest_stay_quiet() {
        for code in [1, 2, 42, 126, 127, 130, 137, 139, 143] {
            assert!(exit_words(code).is_some(), "{code}");
        }
        for code in [0, 3, 41, 43, 128, 255, -1] {
            assert!(exit_words(code).is_none(), "{code}");
        }
        assert!(exit_words(127).unwrap().contains("PATH"));
        assert!(exit_words(130).unwrap().contains("Ctrl+C"));
    }
}
