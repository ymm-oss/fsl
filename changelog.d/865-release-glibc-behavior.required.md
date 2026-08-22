Required (#865): Added accepting and rejecting fixture controls for the Linux
release GLIBC ABI guard. The required product gate now exercises the deployed
guard with compliant GLIBC_2.39 and rejecting GLIBC_2.40 `readelf` fixtures.
