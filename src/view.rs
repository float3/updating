//! What each screen is sent.
//!
//! The facilitator sees everything. A player is sent their own answers,
//! and, once paired, where their partners stand on the statement they were
//! given: that is the conversation. Between rounds they are also sent the
//! few people who disagree with them most, and on what, so they can go
//! and find them without waiting for a round.

use serde::Serialize;

use crate::game::{
    Game, Group, MINUTES, Mode, Persuaded, Phase, Player, SIDES, Scale, Statement, UPDATE,
};
use crate::statements::TOPICS;

#[derive(Serialize)]
pub struct TopicChoice {
    key: &'static str,
    name: &'static str,
    size: usize,
    chosen: bool,
}

#[derive(Serialize)]
pub struct Said {
    name: String,
    before: i16,
    after: Option<i16>,
}

#[derive(Serialize)]
pub struct GroupView<'a> {
    number: usize,
    statement: &'a Statement,
    people: Vec<Said>,
    gap: i16,
}

#[derive(Serialize)]
pub struct RoundView<'a> {
    number: usize,
    groups: Vec<GroupView<'a>>,
    waiting: Vec<String>,
}

#[derive(Serialize)]
pub struct Who {
    name: String,
    answered: usize,
    of: usize,
    /// How many answers moved by at least `UPDATE` since they were given.
    updated: usize,
    moved: i32,
    lines: usize,
    persuaded: Persuaded,
}

/// One line of the persuasion board.
#[derive(Serialize)]
pub struct Leader {
    name: String,
    #[serde(flatten)]
    persuaded: Persuaded,
}

/// Who moved the people they talked to most, best first.
fn leaders(game: &Game) -> Vec<Leader> {
    let scores = game.persuasion();
    let mut out: Vec<Leader> = game
        .players
        .iter()
        .filter_map(|p| {
            scores.get(&p.key).map(|&persuaded| Leader {
                name: p.name.clone(),
                persuaded,
            })
        })
        .collect();
    out.sort_by_key(|l| std::cmp::Reverse((l.persuaded.points, l.persuaded.people)));
    out
}

#[derive(Serialize)]
pub struct Tally<'a> {
    statement: &'a Statement,
    n: usize,
    mean: f64,
    /// The standard deviation, on a common scale from -100 to 100.
    spread: f64,
    first_mean: f64,
    updated: usize,
}

#[derive(Serialize)]
pub struct HostView<'a> {
    join: &'a str,
    mode: Mode,
    scale: Scale,
    topics: Vec<TopicChoice>,
    custom: String,
    side: u8,
    sides: &'static [u8],
    minutes: u32,
    lengths: &'static [u32],
    deck: usize,
    locked: bool,
    phase: Phase,
    ends: u64,
    round: Option<RoundView<'a>>,
    rounds: usize,
    players: Vec<Who>,
    contested: Vec<Tally<'a>>,
    shifted: Vec<Tally<'a>>,
    /// What the most people have answered, which is where a free-for-all
    /// room is converging.
    popular: Vec<Tally<'a>>,
    answers: usize,
    updates: usize,
    leaders: Vec<Leader>,
}

fn name_of(game: &Game, key: &str) -> String {
    game.player(key).map(|p| p.name.clone()).unwrap_or_default()
}

fn group_view<'a>(game: &'a Game, number: usize, group: &Group) -> Option<GroupView<'a>> {
    let statement = game.statement(group.statement)?;
    let people: Vec<Said> = group
        .members
        .iter()
        .map(|m| Said {
            name: name_of(game, m),
            before: group.before.get(m).copied().unwrap_or(0),
            after: group.after.get(m).copied(),
        })
        .collect();
    let gap = people
        .iter()
        .flat_map(|a| {
            people
                .iter()
                .map(move |b| statement.gap(a.before, b.before))
        })
        .max()
        .unwrap_or(0);
    Some(GroupView {
        number,
        statement,
        people,
        gap,
    })
}

/// How many of a player's answers moved far enough to count as an update,
/// and how far they moved in all, on the shared scale of `Statement::gap`.
fn moved(game: &Game, player: &Player) -> (usize, i32) {
    player
        .answers
        .iter()
        .filter_map(|(id, now)| {
            let then = player.first.get(id)?;
            Some(game.statement(*id)?.gap(*now, *then))
        })
        .fold((0, 0), |(n, sum), d| {
            (n + (d >= UPDATE) as usize, sum + d as i32)
        })
}

/// The room's answers to one statement. Means are on the statement's own
/// scale; the spread is on a common one, -100 to 100, so that spreads
/// compare across the two kinds.
fn tally<'a>(game: &'a Game, statement: &'a Statement) -> Option<Tally<'a>> {
    let scale = |v: i16| -> f64 { statement.norm(v) * 100.0 };
    let now: Vec<f64> = game
        .players
        .iter()
        .filter_map(|p| p.answers.get(&statement.id).map(|&v| scale(v)))
        .collect();
    if now.is_empty() {
        return None;
    }
    let firsts: Vec<f64> = game
        .players
        .iter()
        .filter_map(|p| p.first.get(&statement.id).map(|&v| scale(v)))
        .collect();
    let n = now.len() as f64;
    let mean = now.iter().sum::<f64>() / n;
    let spread = (now.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
    let first_mean = firsts.iter().sum::<f64>() / firsts.len().max(1) as f64;
    let updated = game
        .players
        .iter()
        .filter(
            |p| match (p.first.get(&statement.id), p.answers.get(&statement.id)) {
                (Some(a), Some(b)) => statement.gap(*a, *b) >= UPDATE,
                _ => false,
            },
        )
        .count();
    // Back on the statement's own scale, for showing.
    let back = |v: f64| {
        if statement.chance {
            (v + 100.0) / 2.0
        } else {
            v / 100.0 * 3.0 + 4.0
        }
    };
    Some(Tally {
        statement,
        n: now.len(),
        mean: back(mean),
        spread,
        first_mean: back(first_mean),
        updated,
    })
}

pub fn host(game: &Game) -> HostView<'_> {
    let round = game
        .rounds
        .last()
        .filter(|_| matches!(game.phase, Phase::Talking | Phase::Updating))
        .map(|r| RoundView {
            number: game.rounds.len(),
            groups: r
                .groups
                .iter()
                .enumerate()
                .filter_map(|(i, g)| group_view(game, i + 1, g))
                .collect(),
            waiting: r.waiting.iter().map(|k| name_of(game, k)).collect(),
        });
    let scores = game.persuasion();
    let players: Vec<Who> = game
        .players
        .iter()
        .map(|p| {
            let (updated, moved) = moved(game, p);
            Who {
                name: p.name.clone(),
                answered: p.answers.len(),
                of: game.hand(p).len(),
                updated,
                moved,
                lines: game.lines(p),
                persuaded: scores.get(&p.key).copied().unwrap_or_default(),
            }
        })
        .collect();
    let tallies: Vec<Tally> = game.deck.iter().filter_map(|s| tally(game, s)).collect();
    let mut contested: Vec<&Tally> = tallies.iter().filter(|t| t.n >= 2).collect();
    contested.sort_by(|a, b| b.spread.total_cmp(&a.spread));
    let contested_ids: Vec<u32> = contested.iter().take(5).map(|t| t.statement.id).collect();
    let mut shifted: Vec<&Tally> = tallies.iter().filter(|t| t.updated > 0).collect();
    shifted.sort_by_key(|t| std::cmp::Reverse(t.updated));
    let shifted_ids: Vec<u32> = shifted.iter().take(5).map(|t| t.statement.id).collect();
    let mut popular: Vec<&Tally> = tallies.iter().filter(|t| t.n >= 2).collect();
    popular.sort_by_key(|t| std::cmp::Reverse(t.n));
    let popular_ids: Vec<u32> = popular.iter().take(5).map(|t| t.statement.id).collect();
    let pick = |ids: &[u32]| -> Vec<Tally> {
        ids.iter()
            .filter_map(|&id| game.statement(id).and_then(|s| tally(game, s)))
            .collect()
    };
    HostView {
        join: &game.join,
        mode: game.mode,
        scale: game.scale,
        topics: TOPICS
            .iter()
            .map(|t| TopicChoice {
                key: t.key,
                name: t.name,
                size: t.statements.len(),
                chosen: game.topics.iter().any(|k| k == t.key),
            })
            .collect(),
        custom: game.custom.join("\n"),
        side: game.side,
        sides: SIDES,
        minutes: game.minutes,
        lengths: MINUTES,
        deck: game.deck.len(),
        locked: game.locked(),
        phase: game.phase,
        ends: game.ends,
        round,
        rounds: game.rounds.len(),
        answers: game.players.iter().map(|p| p.answers.len()).sum(),
        updates: game.players.iter().map(|p| moved(game, p).0).sum(),
        players,
        leaders: leaders(game),
        contested: pick(&contested_ids),
        shifted: pick(&shifted_ids),
        popular: pick(&popular_ids),
    }
}

#[derive(Serialize)]
pub struct Square<'a> {
    statement: &'a Statement,
    answer: Option<i16>,
    first: Option<i16>,
    marked: bool,
    /// In the free mode, how many other people have answered it.
    count: usize,
    /// In the free mode, and only once this player has answered it: who
    /// else did, and what they said.
    others: Vec<Seen>,
}

#[derive(Serialize)]
pub struct Seen {
    name: String,
    answer: i16,
}

#[derive(Serialize)]
pub struct Partner {
    name: String,
    answer: i16,
    after: Option<i16>,
}

#[derive(Serialize)]
pub struct Mine<'a> {
    number: usize,
    statement: &'a Statement,
    partners: Vec<Partner>,
    before: i16,
    after: Option<i16>,
}

#[derive(Serialize)]
pub struct Opposite<'a> {
    name: String,
    statement: &'a Statement,
    mine: i16,
    theirs: i16,
    gap: i16,
}

#[derive(Serialize)]
pub struct PlayerView<'a> {
    name: String,
    mode: Mode,
    side: u8,
    phase: Phase,
    ends: u64,
    round: usize,
    squares: Vec<Square<'a>>,
    topics: Vec<(&'static str, &'static str)>,
    group: Option<Mine<'a>>,
    waiting: bool,
    opposites: Vec<Opposite<'a>>,
    lines: usize,
    updated: usize,
    moved: i32,
    players: usize,
    persuaded: Persuaded,
    /// In the free mode, the quick-fire feed's next statements, best first.
    next: Vec<u32>,
    /// Their place on the persuasion board, once they have one.
    rank: Option<usize>,
    /// The board's top three, shown when the evening is over.
    winners: Vec<Leader>,
}

/// How many statements ahead the quick-fire feed is sent.
const FEED: usize = 8;

/// How many people a player is shown to go and find.
const OPPOSITES: usize = 5;

pub fn player<'a>(game: &'a Game, key: &str) -> Option<PlayerView<'a>> {
    let me = game.player(key)?;
    let free = game.mode == Mode::Free;
    let squares = game
        .hand(me)
        .into_iter()
        .filter_map(|id| {
            let answer = me.answers.get(&id).copied();
            let mut others: Vec<Seen> = Vec::new();
            let mut count = 0;
            if free {
                for p in game.players.iter().filter(|p| p.key != key) {
                    let Some(&theirs) = p.answers.get(&id) else {
                        continue;
                    };
                    count += 1;
                    // Not before answering: knowing who said what would
                    // anchor the answer.
                    if answer.is_some() {
                        others.push(Seen {
                            name: p.name.clone(),
                            answer: theirs,
                        });
                    }
                }
            }
            Some(Square {
                statement: game.statement(id)?,
                answer,
                first: me.first.get(&id).copied(),
                marked: me.marked.contains(&id),
                count,
                others,
            })
        })
        .collect();
    let group = game.group_of(key).and_then(|(i, g)| {
        Some(Mine {
            number: i + 1,
            statement: game.statement(g.statement)?,
            partners: g
                .members
                .iter()
                .filter(|m| *m != key)
                .map(|m| Partner {
                    name: name_of(game, m),
                    answer: g.before.get(m).copied().unwrap_or(0),
                    after: g.after.get(m).copied(),
                })
                .collect(),
            before: g.before.get(key).copied().unwrap_or(0),
            after: g.after.get(key).copied(),
        })
    });
    let waiting = matches!(game.phase, Phase::Talking | Phase::Updating) && group.is_none();
    let opposites = if game.phase == Phase::Talking {
        Vec::new()
    } else {
        let mut found: Vec<Opposite> = game
            .players
            .iter()
            .filter(|p| p.key != key)
            .filter_map(|p| {
                let (id, gap) = game.best_between(me, p)?;
                Some(Opposite {
                    name: p.name.clone(),
                    statement: game.statement(id)?,
                    mine: me.answers[&id],
                    theirs: p.answers[&id],
                    gap,
                })
            })
            .filter(|o| o.gap > 0)
            .collect();
        found.sort_by_key(|o| std::cmp::Reverse(o.gap));
        found.truncate(OPPOSITES);
        found
    };
    let (updated, moved) = moved(game, me);
    let board = leaders(game);
    let rank = board.iter().position(|l| l.name == me.name).map(|i| i + 1);
    let persuaded = game.persuasion().get(key).copied().unwrap_or_default();
    let winners = if game.phase == Phase::Finished {
        board.into_iter().take(3).collect()
    } else {
        Vec::new()
    };
    Some(PlayerView {
        name: me.name.clone(),
        mode: game.mode,
        side: game.side,
        phase: game.phase,
        ends: game.ends,
        round: game.rounds.len(),
        squares,
        topics: TOPICS
            .iter()
            .map(|t| (t.key, t.name))
            .chain([("custom", "From the facilitator")])
            .collect(),
        group,
        waiting,
        opposites,
        lines: game.lines(me),
        updated,
        moved,
        players: game.players.len(),
        persuaded,
        rank,
        winners,
        next: if free {
            game.feed(me, FEED)
        } else {
            Vec::new()
        },
    })
}
