"""fslc — FSL (AI-Native Formal Spec Language) bounded model checker."""
from .parser import parse
from .model import build_spec, check_spec, FslError
from .bmc import verify, prove, scenarios
from .runtime import Monitor

try:
    from importlib.metadata import version as _pkg_version, PackageNotFoundError
    try:
        __version__ = _pkg_version("fslc")
    except PackageNotFoundError:
        __version__ = "1.0.0"
except Exception:
    __version__ = "1.0.0"

__all__ = [
    "parse", "build_spec", "check_spec", "verify", "prove", "scenarios",
    "Monitor", "FslError", "__version__",
]
