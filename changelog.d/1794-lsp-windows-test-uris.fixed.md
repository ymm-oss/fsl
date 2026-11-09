Fixed (#1794): the `fsl-lsp` stdio tests pass on Windows. Two tests built or
expected Unix-only file URIs: `file://C:\...` is normalized by the server, so
nine `issue_1517` cases waited out their 120 s deadline, and a drive-less
`file:///tmp/...` has no Windows path. The release workflow's Windows MSRV test
step failed on them. Test-only; the server is unchanged.
