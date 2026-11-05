use std::mem::size_of;

pub(super) fn copy_token_user_sid(buffer: &[usize], returned_len: usize) -> Option<Vec<u8>> {
    const WORD: usize = size_of::<usize>();
    const TOKEN_USER_HEADER: usize = WORD * 2;
    if returned_len < TOKEN_USER_HEADER || returned_len > buffer.len().checked_mul(WORD)? {
        return None;
    }
    let sid_address = *buffer.first()?;
    let offset = sid_address.checked_sub(buffer.as_ptr().addr())?;
    if offset < TOKEN_USER_HEADER || offset.checked_add(8)? > returned_len {
        return None;
    }
    let mut header = buffer
        .iter()
        .flat_map(|word| word.to_ne_bytes())
        .skip(offset);
    if header.next()? != 1 {
        return None;
    }
    let authorities = usize::from(header.next()?);
    if authorities > 15 {
        return None;
    }
    let sid_len = 8usize.checked_add(authorities.checked_mul(4)?)?;
    if offset.checked_add(sid_len)? > returned_len {
        return None;
    }
    Some(
        buffer
            .iter()
            .flat_map(|word| word.to_ne_bytes())
            .skip(offset)
            .take(sid_len)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(sid: &[u8]) -> (Vec<usize>, usize) {
        let mut buffer = vec![0usize; 32];
        for (index, chunk) in sid.chunks(size_of::<usize>()).enumerate() {
            let mut bytes = [0; size_of::<usize>()];
            bytes[..chunk.len()].copy_from_slice(chunk);
            buffer[2 + index] = usize::from_ne_bytes(bytes);
        }
        buffer[0] = buffer.as_ptr().addr() + size_of::<usize>() * 2;
        (buffer, size_of::<usize>() * 2 + sid.len())
    }

    #[test]
    fn copies_only_the_sid_from_a_bounded_token_user_buffer() {
        for count in [0u8, 1, 15] {
            let mut sid = vec![0; 8 + usize::from(count) * 4];
            sid[0] = 1;
            sid[1] = count;
            sid[7] = 5;
            let (buffer, used) = token(&sid);
            assert_eq!(copy_token_user_sid(&buffer, used), Some(sid));
        }
    }

    #[test]
    fn refuses_external_truncated_overlapping_and_invalid_sid_layouts() {
        let sid = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
        for case in 0..10 {
            let (mut buffer, mut used) = token(&sid);
            let base = buffer.as_ptr().addr();
            match case {
                0 => used = size_of::<usize>() * 2 - 1,
                1 => used = buffer.len() * size_of::<usize>() + 1,
                2 => buffer[0] = 0,
                3 => buffer[0] = base - 1,
                4 => buffer[0] = base + used - 7,
                5 => buffer[0] = base + size_of::<usize>() * 2 - 1,
                6 | 7 => {
                    let mut bytes = buffer[2].to_ne_bytes();
                    bytes[usize::from(case == 7)] = if case == 6 { 2 } else { 16 };
                    buffer[2] = usize::from_ne_bytes(bytes);
                }
                8 => used -= 1,
                9 => buffer[0] = usize::MAX,
                _ => unreachable!(),
            }
            assert_eq!(copy_token_user_sid(&buffer, used), None, "case {case}");
        }
    }
}
