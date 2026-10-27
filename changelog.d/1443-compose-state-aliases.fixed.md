Fixed (#1443): compose state declarations now resolve component aliases in inline
initializers and type-range expressions. Component constants can initialize a
compose state field or define its bounds. Inline initializers still cannot read
any state root: `seen: Bool = a.on` now reports the existing state-read prohibition
with the resolved public name instead of failing to type the alias identifier.
Qualified type syntax is unchanged.
