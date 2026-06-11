"""fslc — FSL (AI-Native Formal Spec Language) bounded model checker."""
from .parser import parse
from .model import build_spec, check_spec, FslError
from .bmc import verify, prove, scenarios

__version__ = "1.0.0"

__all__ = ["parse", "build_spec", "check_spec", "verify", "prove", "scenarios", "FslError", "__version__"]
