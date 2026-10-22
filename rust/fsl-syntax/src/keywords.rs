// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The English / Japanese keyword table of next-edition requirements, and the
//! particles が・を・に・で・は (fsl 5 plan §3 S5, keyword table proposal
//! range B, decisions/048 J6).
//!
//! fsl has no reserved words: the lexer reads every word as an identifier and
//! the parser compares it with a keyword where the grammar expects one. The
//! table keeps that design. Each row is one keyword in one [`KeywordContext`]
//! (where in the grammar it is read), with its English spelling (one or more
//! words) and its Japanese spelling (one word). In a next-edition
//! requirements document the parser reads a keyword through the table
//! ([`Kw`]), so either spelling is accepted where the English one is, and the
//! two may be mixed in one document. Everywhere else only the English
//! spelling is read, exactly as before.
//!
//! - **Position.** A Japanese keyword stands where the English one does,
//!   except `with` ([`Position::After`]): its Japanese is the particle で
//!   after the input (`申請金額 で`). `is` is は at the English position.
//! - **Per context, one to one.** No two rows of a context share an English
//!   or a Japanese spelling ([`KEYWORDS`]'s tests). One Japanese word may
//!   serve rows of different contexts only when the rows mean the same
//!   ([`SHARED_JAPANESE`]: `条件` is `only when` after `allow` and `when`
//!   after `never`).
//! - **Range B only.** Header, `implements` and its mapping, expression
//!   operators other than `and` / `or`, kernel members and `time` stay
//!   English (proposal §4, §5.8). Reserved rows ([`Status::Reserved`]) are
//!   words a later stage reads; they are listed so that an alias cannot take
//!   them now (`alias_shadows_keyword`).
//!
//! The lookups go both ways ([`english_of`], [`japanese_of`],
//! [`Keyword::lookup_english`], [`Keyword::lookup_japanese`]) for the
//! formatter's language switch and the editor's keyword index.

/// Where in the grammar a keyword is read. Rows of one context are one to
/// one; the same English word may be a different keyword in another context
/// (`in` is a stage after `event … =` and set membership in an expression).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum KeywordContext {
    /// The document's blocks: `requirements`, `glossary`, `verify`.
    Document,
    /// A declaration's leading word, at item position in `requirements`.
    Declaration,
    /// Inside `verify { … }`.
    Verify,
    /// Inside `entity Name { … }`.
    Entity,
    /// `concept Name = Entity [in S…] [where …]`.
    Concept,
    /// `R belongs to E as r via n` (reserved until the relations stage).
    Relation,
    /// `[fault] event name = phrase [with …] S -> T | in S [set …] [covers …]`.
    Event,
    /// Inside a rule's or an event's expression: `input name`.
    RuleExpression,
    /// `allow P only when X | only for C`.
    Allow,
    /// `never P when X | for C`.
    Never,
    /// `P only after Q`.
    After,
    /// `E enters T only from S…`.
    Enters,
    /// `always C1 is C2`, `always E [not] in S… has X`.
    Always,
    /// `deadline name <= bound` inside a rule.
    Deadline,
    /// `decide ID "text" for S E { … [otherwise => …] } [assuming Q]`.
    Decide,
    /// A decision table row's outcome: `Actor verb {or verb}`.
    DecideRow,
    /// `goal ID "text" some E can reach S` (reserved until the goals stage).
    Goal,
    /// `policy ID "text" every E in S must eventually be T {or T}`
    /// (reserved until the goals stage).
    Policy,
    /// `measure` / `kpi name = count E in S`.
    Measure,
    /// `scenario` / `forbidden` bodies: steps and the `then` check.
    Scenario,
    /// The boolean operators of an expression.
    Expression,
}

/// Where the Japanese spelling stands relative to what it marks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Position {
    /// Where the English keyword stands.
    Before,
    /// After the word it marks, as a particle (`with x` → `x で`).
    After,
}

/// Whether the parser reads the row yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    /// Read wherever its English spelling is.
    Active,
    /// Not read yet: listed so that no alias takes the word.
    Reserved,
}

/// One keyword, as the parser asks for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Kw {
    Requirements,
    Glossary,
    Verify,
    Instances,
    Values,
    Assume,
    OutOfScope,
    Type,
    Enum,
    Number,
    Const,
    Actor,
    Entity,
    Concept,
    Resource,
    Lifecycle,
    ConceptIn,
    ConceptWhere,
    BelongsTo,
    RelationAs,
    RelationVia,
    Event,
    Fault,
    Environment,
    EventIn,
    EventSet,
    Covers,
    EventWith,
    Input,
    Rule,
    Allow,
    AllowOnlyWhen,
    AllowOnlyFor,
    AllowOnlyAs,
    Never,
    NeverWhen,
    NeverFor,
    Because,
    OnlyAfter,
    Enters,
    OnlyFrom,
    Always,
    AlwaysIs,
    AlwaysIn,
    AlwaysNotIn,
    AlwaysHas,
    Deadline,
    Decide,
    DecideFor,
    Otherwise,
    Assuming,
    RowOr,
    Goal,
    GoalSome,
    CanReach,
    Policy,
    PolicyEvery,
    PolicyIn,
    MustEventuallyBe,
    PolicyOr,
    Measure,
    Kpi,
    Count,
    MeasureIn,
    Scenario,
    Forbidden,
    Then,
    ThenIs,
    ThenAnd,
    Rejected,
    StepWith,
    And,
    Or,
}

/// One row of the table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Keyword {
    pub kw: Kw,
    pub context: KeywordContext,
    /// The English spelling: words separated by one space (`only when`).
    pub english: &'static str,
    /// The Japanese spelling: one word.
    pub japanese: &'static str,
    pub position: Position,
    pub status: Status,
}

const fn row(
    kw: Kw,
    context: KeywordContext,
    english: &'static str,
    japanese: &'static str,
) -> Keyword {
    Keyword {
        kw,
        context,
        english,
        japanese,
        position: Position::Before,
        status: Status::Active,
    }
}

const fn reserved(
    kw: Kw,
    context: KeywordContext,
    english: &'static str,
    japanese: &'static str,
) -> Keyword {
    Keyword {
        status: Status::Reserved,
        ..row(kw, context, english, japanese)
    }
}

const fn after(
    kw: Kw,
    context: KeywordContext,
    english: &'static str,
    japanese: &'static str,
) -> Keyword {
    Keyword {
        position: Position::After,
        ..row(kw, context, english, japanese)
    }
}

use KeywordContext as C;

/// The table, in the order of the proposal's §5. Rows the parser does not
/// read yet are [`Status::Reserved`]: the relations, `resource`, `measure`,
/// `goal`, `policy` and `assume` / `out of scope` rows are range B but come
/// with their stages; `environment`, `only as` and `because` are reserved by
/// the proposal itself.
pub const KEYWORDS: &[Keyword] = &[
    // §5.1 file and structure
    row(Kw::Requirements, C::Document, "requirements", "要件"),
    row(Kw::Glossary, C::Document, "glossary", "用語集"),
    row(Kw::Verify, C::Document, "verify", "検証"),
    row(Kw::Instances, C::Verify, "instances", "個数"),
    row(Kw::Values, C::Verify, "values", "値域"),
    reserved(Kw::Assume, C::Declaration, "assume", "前提"),
    reserved(Kw::OutOfScope, C::Declaration, "out of scope", "範囲外"),
    // §5.2 vocabulary
    row(Kw::Type, C::Declaration, "type", "型"),
    row(Kw::Enum, C::Declaration, "enum", "列挙"),
    row(Kw::Number, C::Declaration, "number", "数値"),
    row(Kw::Const, C::Declaration, "const", "定数"),
    row(Kw::Actor, C::Declaration, "actor", "アクター"),
    row(Kw::Entity, C::Declaration, "entity", "エンティティ"),
    row(Kw::Lifecycle, C::Entity, "lifecycle", "ライフサイクル"),
    row(Kw::Concept, C::Declaration, "concept", "区分"),
    row(Kw::ConceptIn, C::Concept, "in", "段階"),
    row(Kw::ConceptWhere, C::Concept, "where", "のうち"),
    reserved(Kw::Resource, C::Declaration, "resource", "資源"),
    reserved(Kw::BelongsTo, C::Relation, "belongs to", "所属先"),
    reserved(Kw::RelationAs, C::Relation, "as", "役名"),
    reserved(Kw::RelationVia, C::Relation, "via", "契機"),
    // §5.3 events
    row(Kw::Event, C::Declaration, "event", "出来事"),
    row(Kw::Fault, C::Event, "fault", "異常"),
    reserved(Kw::Environment, C::Event, "environment", "外部"),
    row(Kw::EventIn, C::Event, "in", "段階"),
    row(Kw::EventSet, C::Event, "set", "更新"),
    row(Kw::Covers, C::Event, "covers", "対応要件"),
    after(Kw::EventWith, C::Event, "with", "で"),
    row(Kw::Input, C::RuleExpression, "input", "入力"),
    // §5.4 rules and decision tables
    row(Kw::Rule, C::Declaration, "rule", "規則"),
    row(Kw::Allow, C::Allow, "allow", "許可"),
    row(Kw::AllowOnlyWhen, C::Allow, "only when", "条件"),
    row(Kw::AllowOnlyFor, C::Allow, "only for", "対象"),
    reserved(Kw::AllowOnlyAs, C::Allow, "only as", "立場"),
    row(Kw::Never, C::Never, "never", "禁止"),
    row(Kw::NeverWhen, C::Never, "when", "条件"),
    row(Kw::NeverFor, C::Never, "for", "対象"),
    reserved(Kw::Because, C::Never, "because", "理由"),
    row(Kw::OnlyAfter, C::After, "only after", "先行条件"),
    row(Kw::Enters, C::Enters, "enters", "遷移先"),
    row(Kw::OnlyFrom, C::Enters, "only from", "遷移元"),
    row(Kw::Always, C::Always, "always", "常に"),
    row(Kw::AlwaysIs, C::Always, "is", "は"),
    row(Kw::AlwaysIn, C::Always, "in", "段階"),
    row(Kw::AlwaysNotIn, C::Always, "not in", "段階以外"),
    row(Kw::AlwaysHas, C::Always, "has", "必須"),
    row(Kw::Deadline, C::Deadline, "deadline", "期限"),
    row(Kw::Decide, C::Declaration, "decide", "決定表"),
    row(Kw::DecideFor, C::Decide, "for", "対象"),
    row(Kw::RowOr, C::DecideRow, "or", "または"),
    row(Kw::Otherwise, C::Decide, "otherwise", "それ以外"),
    row(Kw::Assuming, C::Decide, "assuming", "仮定"),
    // §5.5 goals, policies, measures
    reserved(Kw::Goal, C::Declaration, "goal", "目標"),
    reserved(Kw::GoalSome, C::Goal, "some", "いずれかの"),
    reserved(Kw::CanReach, C::Goal, "can reach", "到達可能"),
    reserved(Kw::Policy, C::Declaration, "policy", "方針"),
    reserved(Kw::PolicyEvery, C::Policy, "every", "すべての"),
    reserved(Kw::PolicyIn, C::Policy, "in", "段階"),
    reserved(
        Kw::MustEventuallyBe,
        C::Policy,
        "must eventually be",
        "いずれ必ず",
    ),
    reserved(Kw::PolicyOr, C::Policy, "or", "または"),
    reserved(Kw::Measure, C::Declaration, "measure", "指標"),
    row(Kw::Kpi, C::Declaration, "kpi", "重要指標"),
    row(Kw::Count, C::Measure, "count", "件数"),
    row(Kw::MeasureIn, C::Measure, "in", "段階"),
    // §5.6 scenarios
    row(Kw::Scenario, C::Declaration, "scenario", "シナリオ"),
    row(Kw::Forbidden, C::Declaration, "forbidden", "拒否シナリオ"),
    row(Kw::Then, C::Scenario, "then", "結果"),
    row(Kw::ThenIs, C::Scenario, "is", "は"),
    row(Kw::ThenAnd, C::Scenario, "and", "かつ"),
    row(Kw::Rejected, C::Scenario, "rejected", "拒否"),
    after(Kw::StepWith, C::Scenario, "with", "で"),
    // §5.7 expressions
    row(Kw::And, C::Expression, "and", "かつ"),
    row(Kw::Or, C::Expression, "or", "または"),
];

/// Japanese words that serve rows of more than one context with different
/// English spellings, each because the rows mean the same (proposal §2-3):
/// `(japanese, the English spellings it stands for)`. A test keeps the table
/// from sharing any other word.
pub const SHARED_JAPANESE: &[(&str, &[&str])] = &[
    // "under this condition": allow's `only when`, never's `when`.
    ("条件", &["only when", "when"]),
    // "the instances it applies to": allow's `only for`, never's `for`, the
    // decision table's `for S E`.
    ("対象", &["only for", "for"]),
];

/// A particle: a word after another that names the other's role in a phrase
/// (event declaration, rule, decision table row, scenario step). With
/// particles the words of a phrase may come in any order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Particle {
    /// が: the actor.
    Ga,
    /// を: the entity (or, in a scenario, the instance).
    Wo,
    /// に: the counterpart. No phrase of this edition has one.
    Ni,
    /// で: an input (the Japanese of `with`).
    De,
    /// は: the topic of a check (`then x は S`, `always C1 は C2`).
    Wa,
}

/// Every particle with its spelling.
pub const PARTICLES: &[(Particle, &str)] = &[
    (Particle::Ga, "が"),
    (Particle::Wo, "を"),
    (Particle::Ni, "に"),
    (Particle::De, "で"),
    (Particle::Wa, "は"),
];

impl Particle {
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Ga => "が",
            Self::Wo => "を",
            Self::Ni => "に",
            Self::De => "で",
            Self::Wa => "は",
        }
    }

    /// The role the particle gives the word before it.
    #[must_use]
    pub const fn role(self) -> &'static str {
        match self {
            Self::Ga => "actor",
            Self::Wo => "entity",
            Self::Ni => "counterpart",
            Self::De => "input",
            Self::Wa => "topic",
        }
    }
}

/// The particle spelled `word`.
#[must_use]
pub fn particle(word: &str) -> Option<Particle> {
    PARTICLES
        .iter()
        .find(|(_, spelling)| *spelling == word)
        .map(|(particle, _)| *particle)
}

impl Kw {
    /// The keyword's row.
    ///
    /// # Panics
    ///
    /// Never: every keyword has exactly one row
    /// (`every_keyword_has_exactly_one_row`).
    #[must_use]
    pub fn entry(self) -> &'static Keyword {
        KEYWORDS
            .iter()
            .find(|keyword| keyword.kw == self)
            .expect("every keyword has a row")
    }

    #[must_use]
    pub fn english(self) -> &'static str {
        self.entry().english
    }

    #[must_use]
    pub fn japanese(self) -> &'static str {
        self.entry().japanese
    }
}

impl Keyword {
    /// The row of `context` spelled `english` (words separated by one space).
    #[must_use]
    pub fn lookup_english(context: KeywordContext, english: &str) -> Option<&'static Self> {
        KEYWORDS
            .iter()
            .find(|keyword| keyword.context == context && keyword.english == english)
    }

    /// The row of `context` spelled `japanese`.
    #[must_use]
    pub fn lookup_japanese(context: KeywordContext, japanese: &str) -> Option<&'static Self> {
        KEYWORDS
            .iter()
            .find(|keyword| keyword.context == context && keyword.japanese == japanese)
    }
}

/// The English spelling of the Japanese keyword `japanese` of `context`.
#[must_use]
pub fn english_of(context: KeywordContext, japanese: &str) -> Option<&'static str> {
    Keyword::lookup_japanese(context, japanese).map(|keyword| keyword.english)
}

/// The Japanese spelling of the English keyword `english` of `context`.
#[must_use]
pub fn japanese_of(context: KeywordContext, english: &str) -> Option<&'static str> {
    Keyword::lookup_english(context, english).map(|keyword| keyword.japanese)
}

/// Whether the one word `word` spells the keyword `kw` in English or in
/// Japanese. For the one-word keywords that a token scan (rather than the
/// parser) recognises, in a next-edition document only.
pub(crate) fn spells(word: &str, kw: Kw) -> bool {
    let entry = kw.entry();
    word == entry.english || word == entry.japanese
}

/// Whether `word` is a Japanese keyword of any context (reserved rows
/// included) or a particle: the words an alias may not be spelled as, beside
/// the English keyword spellings.
#[must_use]
pub fn is_japanese_spelling(word: &str) -> bool {
    particle(word).is_some() || KEYWORDS.iter().any(|keyword| keyword.japanese == word)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    #[test]
    fn every_keyword_has_exactly_one_row() {
        let mut seen = BTreeSet::new();
        for keyword in KEYWORDS {
            assert!(seen.insert(keyword.kw), "{:?} has two rows", keyword.kw);
            assert_eq!(keyword.kw.entry(), keyword);
        }
    }

    /// Within one context, no English and no Japanese spelling names two
    /// rows, so each spelling translates one way.
    #[test]
    fn each_context_is_one_to_one() {
        let mut english = BTreeMap::new();
        let mut japanese = BTreeMap::new();
        for keyword in KEYWORDS {
            if let Some(other) = english.insert((keyword.context, keyword.english), keyword.kw) {
                panic!(
                    "{:?}: '{}' is both {other:?} and {:?}",
                    keyword.context, keyword.english, keyword.kw
                );
            }
            if let Some(other) = japanese.insert((keyword.context, keyword.japanese), keyword.kw) {
                panic!(
                    "{:?}: '{}' is both {other:?} and {:?}",
                    keyword.context, keyword.japanese, keyword.kw
                );
            }
            assert_eq!(
                japanese_of(keyword.context, keyword.english),
                Some(keyword.japanese)
            );
            assert_eq!(
                english_of(keyword.context, keyword.japanese),
                Some(keyword.english)
            );
        }
    }

    /// A Japanese keyword is one word and never a particle, so a particle is
    /// always read as one. `は` is the one intended exception: it is `is`
    /// itself (decisions/045), at `is`'s position.
    #[test]
    fn no_japanese_keyword_is_a_particle_except_is() {
        for keyword in KEYWORDS {
            assert!(
                !keyword.japanese.is_empty()
                    && !keyword.japanese.chars().any(char::is_whitespace)
                    && !keyword.japanese.is_ascii(),
                "{:?}: '{}' is not one Japanese word",
                keyword.kw,
                keyword.japanese
            );
            match particle(keyword.japanese) {
                None => {}
                Some(Particle::Wa) => assert_eq!(keyword.english, "is", "{:?}", keyword.kw),
                Some(Particle::De) => {
                    assert_eq!(
                        (keyword.english, keyword.position),
                        ("with", Position::After),
                        "{:?}",
                        keyword.kw
                    );
                }
                Some(found) => panic!("{:?} is spelled as the particle {found:?}", keyword.kw),
            }
        }
    }

    /// The collision rule (proposal §2-3, §3): a Japanese word that stands
    /// for different English words in different contexts is one of the
    /// listed shared words, and stands for exactly the listed English words.
    /// Every other Japanese word means one English word wherever it is read.
    #[test]
    fn a_japanese_word_shared_across_contexts_means_the_same() {
        let mut meanings = BTreeMap::<&str, BTreeSet<&str>>::new();
        for keyword in KEYWORDS {
            meanings
                .entry(keyword.japanese)
                .or_default()
                .insert(keyword.english);
        }
        for (japanese, english) in &meanings {
            let listed = SHARED_JAPANESE
                .iter()
                .find(|(shared, _)| shared == japanese)
                .map(|(_, english)| english.iter().copied().collect::<BTreeSet<_>>());
            match listed {
                Some(listed) => assert_eq!(&listed, english, "{japanese}"),
                None => assert_eq!(
                    english.len(),
                    1,
                    "'{japanese}' stands for {english:?}; list it in SHARED_JAPANESE or \
                     choose another word"
                ),
            }
        }
        for (japanese, _) in SHARED_JAPANESE {
            assert!(
                meanings.contains_key(japanese),
                "{japanese} is not in the table"
            );
        }
    }

    /// The other half of the collision rule: no Japanese keyword is spelled
    /// like an English keyword of any context, so the two spellings of the
    /// table never read as each other.
    #[test]
    fn no_japanese_keyword_is_spelled_like_an_english_one() {
        let english = KEYWORDS
            .iter()
            .flat_map(|keyword| keyword.english.split(' '))
            .collect::<BTreeSet<_>>();
        for keyword in KEYWORDS {
            assert!(
                !english.contains(keyword.japanese),
                "{:?}: '{}'",
                keyword.kw,
                keyword.japanese
            );
        }
    }

    #[test]
    fn particles_have_one_spelling_each() {
        let spellings = PARTICLES
            .iter()
            .map(|(_, spelling)| *spelling)
            .collect::<BTreeSet<_>>();
        assert_eq!(spellings.len(), PARTICLES.len());
        for (particle, spelling) in PARTICLES {
            assert_eq!(super::particle(spelling), Some(*particle));
            assert_eq!(particle.spelling(), *spelling);
            assert_eq!(spelling.chars().count(), 1);
        }
    }
}
