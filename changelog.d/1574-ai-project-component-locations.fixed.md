Fixed (#1574): embedded AI component diagnostics now use original-document
coordinates across AI check, compatibility, replay, native check and shared
Worker checking. Nested locations and annotation byte spans are translated once;
standalone locations, parse errors, validity, error kinds and exits are preserved.
