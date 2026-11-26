pub(super) fn compress(raw: &[u8]) -> Result<Vec<u8>, Fault> {
    use ::std::io::Write as _;
    let mut encoder =
        ::flate2::write::ZlibEncoder::new(Vec::new(), ::flate2::Compression::default());
    encoder.write_all(raw).map_err(|_| Fault::Representation)?;
    encoder.finish().map_err(|_| Fault::Representation)
}

pub(super) fn decompress(frame: &Frame<'_>, negotiated: bool) -> Result<Vec<u8>, Fault> {
    if !negotiated {
        return Err(Fault::Representation);
    }
    let Some(&flags) = frame.payload.get(1) else {
        return Err(Fault::Representation);
    };
    // RFC 1950 reserves bit 5 of FLG for a preset dictionary.
    if flags & (1 << 5) != 0 {
        return Err(Fault::Representation);
    }

    // A sentinel byte distinguishes full-length output from an overrun.
    let length = frame.size as usize;
    let mut output = vec![0; length + 1];
    let mut stream = ::flate2::Decompress::new(true);
    let status = stream
        .decompress(
            frame.payload,
            &mut output,
            ::flate2::FlushDecompress::Finish,
        )
        .map_err(|_| Fault::Representation)?;
    let exact_member =
        status == ::flate2::Status::StreamEnd && stream.total_in() == frame.payload.len() as u64;
    if !exact_member || stream.total_out() != u64::from(frame.size) {
        return Err(Fault::Representation);
    }
    output.truncate(length);
    Ok(output)
}
