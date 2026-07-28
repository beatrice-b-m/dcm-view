use crate::types::TransferSyntaxClass;

pub fn classify_transfer_syntax(uid: &str) -> TransferSyntaxClass {
    match uid {
        // Browser-renderable lossy JPEG: Baseline, Extended
        "1.2.840.10008.1.2.4.50" | "1.2.840.10008.1.2.4.51" => TransferSyntaxClass::Jpeg,
        // JPEG Lossless: browsers cannot decode — must be decoded server-side
        "1.2.840.10008.1.2.4.57" | "1.2.840.10008.1.2.4.70" => TransferSyntaxClass::JpegLossless,
        "1.2.840.10008.1.2.4.90" | "1.2.840.10008.1.2.4.91" => TransferSyntaxClass::Jpeg2000,
        "1.2.840.10008.1.2" | "1.2.840.10008.1.2.1" | "1.2.840.10008.1.2.2" => {
            TransferSyntaxClass::Uncompressed
        }
        "1.2.840.10008.1.2.4.80" | "1.2.840.10008.1.2.4.81" => TransferSyntaxClass::JpegLs,
        "1.2.840.10008.1.2.5" => TransferSyntaxClass::Rle,
        _ => TransferSyntaxClass::Unsupported,
    }
}
