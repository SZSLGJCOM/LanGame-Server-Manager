from pathlib import Path
import stat


class SourceReadError(RuntimeError):
    def __init__(self, rule: str) -> None:
        super().__init__(rule)
        self.rule = rule


def source_is_present(path: Path) -> bool:
    try:
        path.lstat()
    except FileNotFoundError:
        return False
    except OSError:
        # Keep inaccessible candidates so the scan reports its incomplete coverage.
        return True
    return True


def read_source_payload(path: Path, *, repository_root: Path | None = None) -> bytes:
    try:
        if path.is_symlink():
            raise SourceReadError("non-regular-source-file")
        if repository_root is not None and not path.resolve().is_relative_to(
            repository_root.resolve()
        ):
            raise SourceReadError("unsafe-source-path")
        if not stat.S_ISREG(path.lstat().st_mode):
            raise SourceReadError("non-regular-source-file")
        return path.read_bytes()
    except OSError:
        # The exception may contain target paths or other private operating-system details.
        raise SourceReadError("unreadable-source-file") from None
