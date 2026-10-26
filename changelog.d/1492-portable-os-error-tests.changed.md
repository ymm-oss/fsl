Changed (#1492): ledger ancillary I/O failure tests derive expected messages
from the platform's `std::io::Error` for the same missing path, so the tests
also pass on Windows. Command behavior is unchanged.
