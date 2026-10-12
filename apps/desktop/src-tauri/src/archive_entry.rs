use std::borrow::Cow;
use std::io::Read;

use zip::read::ZipFile;
use zip::result::{ZipError, ZipResult};

pub(crate) fn name<'a, R: Read + ?Sized>(entry: &'a ZipFile<'_, R>) -> ZipResult<Cow<'a, str>> {
    const UTF8_FLAG: u16 = 1 << 11;
    // ZIP's language-encoding flag requires UTF-8. zip 9 otherwise falls back
    // to CP437 even when that flag is set, silently changing malformed names.
    if entry.flags().as_u16() & UTF8_FLAG != 0 {
        return std::str::from_utf8(entry.name_raw())
            .map(Cow::Borrowed)
            .map_err(|error| {
                ZipError::InvalidArchive(format!("entry declares invalid UTF-8: {error}").into())
            });
    }
    entry.name()
}
