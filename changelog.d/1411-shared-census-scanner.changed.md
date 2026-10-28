Changed (#1411): Share one item-level scanner across source censuses that skips only test-only items, so product code after an inline `#[cfg(test)]` item is counted; assertions are unchanged.
