//! Speed updating: everyone says where they stand, then talks to whoever
//! stands furthest away.
//!
//! The facilitator picks topics. Each person either gets a bingo card, a
//! grid of statements dealt to them, or answers as many statements from
//! the room's deck as they like. An answer is agreement from -100 to 100,
//! or, for a question asking for a probability, a chance from 0 to 100.
//!
//! In a round the server pairs everybody with the person they disagree
//! with most, on the statement they are furthest apart on, and a timer
//! runs. When it runs out everybody says where they stand on that
//! statement now. The difference is what the evening is for.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use crate::statements::{TOPICS, topic};

pub const MAX_PLAYERS: usize = 200;
pub const MAX_ROUNDS: usize = 100;
pub const MAX_NAME: usize = 32;
pub const MAX_STATEMENT: usize = 200;
pub const MAX_CUSTOM: usize = 60;
/// The sides a bingo card can have.
pub const SIDES: &[u8] = &[3, 4, 5];
/// Round lengths the facilitator can pick, in minutes.
pub const MINUTES: &[u32] = &[2, 3, 4, 5, 7, 10];
/// How much a round is extended by.
pub const EXTRA_MS: u64 = 60_000;
/// How far an answer has to move to count as an update.
pub const UPDATE: i16 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Everybody gets a grid of statements and marks the ones they talked
    /// about, for a line.
    Bingo,
    /// Everybody answers whatever they like from the whole deck.
    Open,
}

/// How the room answers its statements. Most can be put either way, so
/// the facilitator chooses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scale {
    /// Each as the bank has it: mostly agreement, some probabilities.
    #[default]
    Written,
    /// Everything as agreement; probability questions become claims.
    Agree,
    /// Everything as the chance that it is true.
    Chance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// Answering, and finding people by hand.
    Answering,
    /// A round is on: everybody is talking to who they were paired with.
    Talking,
    /// The round is over; everybody says where they stand now.
    Updating,
    /// The evening is over.
    Finished,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Statement {
    pub id: u32,
    pub text: String,
    pub topic: String,
    /// Answered as a probability, 0 to 100, rather than agreement.
    #[serde(default)]
    pub chance: bool,
}

impl Statement {
    pub fn range(&self) -> (i16, i16) {
        if self.chance { (0, 100) } else { (-100, 100) }
    }

    /// How far apart two answers are, on one scale for both kinds: a
    /// probability's range is half as wide, so its gaps count double.
    pub fn gap(&self, a: i16, b: i16) -> i16 {
        let d = (a - b).abs();
        if self.chance { d * 2 } else { d }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Player {
    pub key: String,
    pub name: String,
    /// A bingo card's statements, row by row. Empty in the open mode,
    /// where the whole deck is everybody's.
    pub card: Vec<u32>,
    /// Where they stand now, by statement.
    pub answers: BTreeMap<u32, i16>,
    /// Where they stood the first time they answered.
    pub first: BTreeMap<u32, i16>,
    /// Statements they have talked about, in a round or on their own.
    pub marked: BTreeSet<u32>,
}

/// Two people, sometimes three, and what they were given to talk about.
#[derive(Serialize, Deserialize)]
pub struct Group {
    pub members: Vec<String>,
    pub statement: u32,
    /// Where each stood when the round started.
    pub before: BTreeMap<String, i16>,
    /// Where each said they stood after it.
    pub after: BTreeMap<String, i16>,
}

/// What somebody's conversations did to the people they talked to.
#[derive(Clone, Copy, Default, Serialize)]
pub struct Persuaded {
    /// Points their partners moved towards them, on the agreement scale.
    pub points: i32,
    /// How many times a partner moved towards them at all.
    pub people: u32,
}

#[derive(Serialize, Deserialize)]
pub struct Round {
    pub started: u64,
    pub groups: Vec<Group>,
    /// Who could not be paired: nobody left, or nothing answered in common.
    pub waiting: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Game {
    pub join: String,
    pub mode: Mode,
    pub topics: Vec<String>,
    pub custom: Vec<String>,
    /// How statements are answered. Kept apart from the deck so the deck
    /// can be built again from it.
    #[serde(default)]
    pub scale: Scale,
    pub side: u8,
    pub minutes: u32,
    pub deck: Vec<Statement>,
    pub players: Vec<Player>,
    pub phase: Phase,
    /// When the round being talked ends.
    pub ends: u64,
    pub rounds: Vec<Round>,
    pub last_seen: u64,
    /// Seeds the dealing, so two rooms deal differently.
    seed: u64,
}

impl rooms::Room for Game {
    fn seen(&mut self, now: u64) {
        self.last_seen = now;
    }

    /// Ends a round whose time is up.
    fn tick(&mut self, now: u64) -> bool {
        if self.phase == Phase::Talking && now >= self.ends {
            self.phase = Phase::Updating;
            return true;
        }
        false
    }

    fn expired(&self, now: u64) -> bool {
        self.last_seen + rooms::DAY_MS < now
    }
}

/// Trims, collapses whitespace, and cuts to `max` characters.
pub fn tidy(text: &str, max: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

/// A small generator for dealing. Nothing here needs to be unpredictable,
/// only different from room to room and person to person.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
    })
}

/// "What is the chance that a fish feels pain?" as a claim to agree with:
/// "A fish feels pain." A question that is not in that form is left as it
/// is.
pub fn claim(question: &str) -> String {
    let Some(rest) = question
        .strip_prefix("What is the chance that ")
        .and_then(|r| r.strip_suffix('?'))
    else {
        return question.to_string();
    };
    let mut chars = rest.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => question.to_string(),
    }
}

/// One statement as the room answers it: as written, or all as agreement,
/// or all as a probability.
fn shaped(text: &str, chance: bool, scale: Scale) -> (String, bool) {
    match (scale, chance) {
        (Scale::Agree, true) => (claim(text), false),
        (Scale::Chance, _) => (text.to_string(), true),
        _ => (text.to_string(), chance),
    }
}

/// Builds a deck from the bank and the facilitator's own statements. A
/// custom line starting with "%" asks for a probability.
fn statements(topics: &[String], custom: &[String], scale: Scale) -> Vec<Statement> {
    let mut deck = Vec::new();
    for key in topics {
        let Some(t) = topic(key) else { continue };
        for text in t.statements {
            let (chance, text) = match text.strip_prefix("% ") {
                Some(rest) => (true, rest),
                None => (false, *text),
            };
            let (text, chance) = shaped(text, chance, scale);
            deck.push(Statement {
                id: deck.len() as u32,
                text,
                topic: t.key.to_string(),
                chance,
            });
        }
    }
    for line in custom {
        let (chance, text) = match line.strip_prefix('%') {
            Some(rest) => (true, rest),
            None => (false, line.as_str()),
        };
        let text = tidy(text, MAX_STATEMENT);
        if !text.is_empty() {
            let (text, chance) = shaped(&text, chance, scale);
            deck.push(Statement {
                id: deck.len() as u32,
                text,
                topic: "custom".into(),
                chance,
            });
        }
    }
    deck
}

impl Game {
    pub fn new(now: u64) -> Game {
        let join = rooms::key::new();
        let seed = hash(&join) ^ now;
        let topics: Vec<String> = TOPICS
            .iter()
            .filter(|t| t.default)
            .map(|t| t.key.to_string())
            .collect();
        let mut game = Game {
            join,
            mode: Mode::Bingo,
            deck: Vec::new(),
            topics,
            custom: Vec::new(),
            scale: Scale::Written,
            side: 4,
            minutes: 5,
            players: Vec::new(),
            phase: Phase::Answering,
            ends: 0,
            rounds: Vec::new(),
            last_seen: now,
            seed,
        };
        game.build_deck();
        game
    }

    pub fn player(&self, key: &str) -> Option<&Player> {
        self.players.iter().find(|p| p.key == key)
    }

    fn player_mut(&mut self, key: &str) -> Result<&mut Player, &'static str> {
        self.players
            .iter_mut()
            .find(|p| p.key == key)
            .ok_or("You are not in this room.")
    }

    pub fn statement(&self, id: u32) -> Option<&Statement> {
        self.deck.get(id as usize)
    }

    /// The statements this player can answer: their card, or the deck.
    pub fn hand(&self, player: &Player) -> Vec<u32> {
        match self.mode {
            Mode::Bingo => player.card.clone(),
            Mode::Open => self.deck.iter().map(|s| s.id).collect(),
        }
    }

    /// Whether anybody has answered anything. After that the deck stays as
    /// it is, because the answers refer to it.
    pub fn locked(&self) -> bool {
        !self.rounds.is_empty() || self.players.iter().any(|p| !p.first.is_empty())
    }

    /// The deck the room plays with. A bingo room takes a sample about
    /// twice the size of a card, so that cards overlap enough for people
    /// to have statements in common; the open mode takes everything.
    fn build_deck(&mut self) {
        let all = statements(&self.topics, &self.custom, self.scale);
        let customs: Vec<&Statement> = all.iter().filter(|s| s.topic == "custom").collect();
        let mut picked: Vec<Statement> = match self.mode {
            Mode::Open => all.clone(),
            Mode::Bingo => {
                let want = (self.side as usize).pow(2) * 2;
                let mut bank: Vec<&Statement> =
                    all.iter().filter(|s| s.topic != "custom").collect();
                let mut r = self.seed;
                // Fisher-Yates, so the sample is fair across topics.
                for i in (1..bank.len()).rev() {
                    r = mix(r);
                    bank.swap(i, (r % (i as u64 + 1)) as usize);
                }
                let room = want.saturating_sub(customs.len());
                let mut chosen: Vec<Statement> = bank.into_iter().take(room).cloned().collect();
                chosen.extend(customs.into_iter().cloned());
                chosen
            }
        };
        for (i, s) in picked.iter_mut().enumerate() {
            s.id = i as u32;
        }
        self.deck = picked;
        let keys: Vec<String> = self.players.iter().map(|p| p.key.clone()).collect();
        for p in &mut self.players {
            p.card.clear();
        }
        for key in keys {
            self.deal(&key);
        }
    }

    /// Deals a card, favouring the statements fewest other cards have, so
    /// that every statement is somebody's and cards overlap evenly.
    fn deal(&mut self, key: &str) {
        if self.mode != Mode::Bingo {
            return;
        }
        let size = (self.side as usize).pow(2).min(self.deck.len());
        let mut used = vec![0usize; self.deck.len()];
        for p in &self.players {
            for &id in &p.card {
                used[id as usize] += 1;
            }
        }
        let mut r = mix(self.seed ^ hash(key));
        let mut order: Vec<(usize, u64, u32)> = (0..self.deck.len())
            .map(|i| {
                r = mix(r);
                (used[i], r, i as u32)
            })
            .collect();
        order.sort();
        let mut card: Vec<u32> = order.into_iter().take(size).map(|(_, _, id)| id).collect();
        // Shuffled again so the grid does not put the rarest in a corner.
        for i in (1..card.len()).rev() {
            r = mix(r);
            card.swap(i, (r % (i as u64 + 1)) as usize);
        }
        if let Some(p) = self.players.iter_mut().find(|p| p.key == key) {
            p.card = card;
        }
    }

    /// Changes what the room plays with, until anybody has answered.
    pub fn setup(
        &mut self,
        mode: Mode,
        scale: Scale,
        topics: &[String],
        custom: &str,
        side: u8,
    ) -> Result<(), &'static str> {
        if self.locked() {
            return Err("People have started answering, so the statements stay as they are.");
        }
        if !SIDES.contains(&side) {
            return Err("A card is 3, 4 or 5 squares wide.");
        }
        let topics: Vec<String> = TOPICS
            .iter()
            .filter(|t| topics.iter().any(|k| k == t.key))
            .map(|t| t.key.to_string())
            .collect();
        let custom: Vec<String> = custom
            .lines()
            .map(|l| tidy(l, MAX_STATEMENT + 1))
            .filter(|l| !l.is_empty())
            .take(MAX_CUSTOM)
            .collect();
        let size = statements(&topics, &custom, scale).len();
        if size == 0 {
            return Err("Pick at least one topic, or write some statements.");
        }
        if mode == Mode::Bingo && size < (side as usize).pow(2) {
            return Err("There are not enough statements for a card that size.");
        }
        self.mode = mode;
        self.scale = scale;
        self.topics = topics;
        self.custom = custom;
        self.side = side;
        self.build_deck();
        Ok(())
    }

    pub fn set_minutes(&mut self, minutes: u32) -> Result<(), &'static str> {
        if !MINUTES.contains(&minutes) {
            return Err("That is not one of the round lengths.");
        }
        self.minutes = minutes;
        Ok(())
    }

    /// Takes a name and returns the key the phone keeps. A name already in
    /// the room gets that person's key back, so a phone that lost its key
    /// can rejoin.
    pub fn join(&mut self, name: &str, now: u64) -> Result<String, &'static str> {
        let name = tidy(name, MAX_NAME);
        if name.is_empty() {
            return Err("Please give a name, so people can find you.");
        }
        if let Some(p) = self.players.iter().find(|p| p.name == name) {
            return Ok(p.key.clone());
        }
        if self.players.len() >= MAX_PLAYERS {
            return Err("This room is full.");
        }
        let key = rooms::key::new();
        self.players.push(Player {
            key: key.clone(),
            name,
            card: Vec::new(),
            answers: BTreeMap::new(),
            first: BTreeMap::new(),
            marked: BTreeSet::new(),
        });
        self.deal(&key);
        self.last_seen = now;
        Ok(key)
    }

    /// Records where somebody stands, or clears it. During and after a
    /// round, an answer to the statement they were given counts as their
    /// answer after the conversation.
    pub fn answer(&mut self, key: &str, id: u32, value: Option<i16>) -> Result<(), &'static str> {
        if self.phase == Phase::Finished {
            return Err("The evening is over.");
        }
        let Some(statement) = self.statement(id).cloned() else {
            return Err("That statement is not in this room.");
        };
        let mode = self.mode;
        let player = self.player_mut(key)?;
        if mode == Mode::Bingo && !player.card.contains(&id) {
            return Err("That statement is not on your card.");
        }
        match value {
            Some(v) => {
                let (low, high) = statement.range();
                if v < low || v > high {
                    return Err("That answer is off the scale.");
                }
                player.answers.insert(id, v);
                player.first.entry(id).or_insert(v);
            }
            None => {
                player.answers.remove(&id);
            }
        }
        if matches!(self.phase, Phase::Talking | Phase::Updating)
            && let Some(round) = self.rounds.last_mut()
            && let Some(group) = round
                .groups
                .iter_mut()
                .find(|g| g.statement == id && g.members.iter().any(|m| m == key))
        {
            match value {
                Some(v) => group.after.insert(key.to_string(), v),
                None => group.after.remove(key),
            };
        }
        Ok(())
    }

    /// Marks a statement as talked about, or unmarks it.
    pub fn mark(&mut self, key: &str, id: u32, marked: bool) -> Result<(), &'static str> {
        if self.statement(id).is_none() {
            return Err("That statement is not in this room.");
        }
        let player = self.player_mut(key)?;
        if marked {
            player.marked.insert(id);
        } else {
            player.marked.remove(&id);
        }
        Ok(())
    }

    /// Pairs everybody up and starts the clock.
    pub fn start_round(&mut self, now: u64) -> Result<(), &'static str> {
        if self.phase == Phase::Talking {
            return Err("A round is already on.");
        }
        if self.rounds.len() >= MAX_ROUNDS {
            return Err("That is as many rounds as one room holds.");
        }
        let met: HashSet<(String, String)> = self
            .rounds
            .iter()
            .flat_map(|r| r.groups.iter())
            .flat_map(|g| {
                g.members.iter().flat_map(move |a| {
                    g.members
                        .iter()
                        .filter(move |b| *b != a)
                        .map(move |b| (a.clone(), b.clone()))
                })
            })
            .collect();
        let (groups, waiting) = pair(self, &met);
        if groups.is_empty() {
            return Err(
                "Nobody can be paired yet: people need to answer some of the same statements.",
            );
        }
        for g in &groups {
            for m in &g.members {
                if let Some(p) = self.players.iter_mut().find(|p| &p.key == m) {
                    p.marked.insert(g.statement);
                }
            }
        }
        self.rounds.push(Round {
            started: now,
            groups,
            waiting,
        });
        self.phase = Phase::Talking;
        self.ends = now + self.minutes as u64 * 60_000;
        Ok(())
    }

    /// Ends the round now rather than when the clock runs out.
    pub fn stop(&mut self) -> Result<(), &'static str> {
        if self.phase != Phase::Talking {
            return Err("No round is on.");
        }
        self.phase = Phase::Updating;
        Ok(())
    }

    pub fn extend(&mut self, now: u64) -> Result<(), &'static str> {
        match self.phase {
            Phase::Talking => self.ends += EXTRA_MS,
            // A round that has just run out can be given its minute back.
            Phase::Updating if !self.rounds.is_empty() => {
                self.phase = Phase::Talking;
                self.ends = now + EXTRA_MS;
            }
            _ => return Err("No round is on."),
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), &'static str> {
        self.phase = Phase::Finished;
        Ok(())
    }

    /// Back from the end, for an evening that was not over after all.
    pub fn reopen(&mut self) -> Result<(), &'static str> {
        if self.phase != Phase::Finished {
            return Err("The evening is still on.");
        }
        self.phase = Phase::Answering;
        Ok(())
    }

    /// The group this player is in this round, if a round has been run.
    pub fn group_of(&self, key: &str) -> Option<(usize, &Group)> {
        if !matches!(self.phase, Phase::Talking | Phase::Updating) {
            return None;
        }
        self.rounds
            .last()?
            .groups
            .iter()
            .enumerate()
            .find(|(_, g)| g.members.iter().any(|m| m == key))
    }

    /// The statement two people disagree on most, and by how much,
    /// preferring one neither has talked about yet.
    pub fn best_between(&self, a: &Player, b: &Player) -> Option<(u32, i16)> {
        let mut best: Option<(i32, u32, i16)> = None;
        for (&id, &x) in &a.answers {
            let Some(&y) = b.answers.get(&id) else {
                continue;
            };
            let Some(s) = self.statement(id) else {
                continue;
            };
            let gap = s.gap(x, y);
            let fresh = !a.marked.contains(&id) && !b.marked.contains(&id);
            let score = gap as i32 + if fresh { 1000 } else { 0 };
            if best.is_none_or(|(s, _, _)| score > s) {
                best = Some((score, id, gap));
            }
        }
        best.map(|(_, id, gap)| (id, gap))
    }

    /// How far each person moved the people they were paired with, over
    /// every round: the points somebody came towards your answer on the
    /// statement you were given. Moving away earns the partner nothing,
    /// and your own movement never counts for you, so the only way to
    /// score is to change somebody else's mind. By player key.
    pub fn persuasion(&self) -> BTreeMap<String, Persuaded> {
        let mut out: BTreeMap<String, Persuaded> = BTreeMap::new();
        for round in &self.rounds {
            for g in &round.groups {
                let Some(s) = self.statement(g.statement) else {
                    continue;
                };
                for mover in &g.members {
                    let (Some(&from), Some(&to)) = (g.before.get(mover), g.after.get(mover)) else {
                        continue;
                    };
                    for other in g.members.iter().filter(|o| *o != mover) {
                        let Some(&there) = g.before.get(other) else {
                            continue;
                        };
                        let closer = s.gap(from, there) - s.gap(to, there);
                        if closer > 0 {
                            let p = out.entry(other.clone()).or_default();
                            p.points += closer as i32;
                            p.people += 1;
                        }
                    }
                }
            }
        }
        out
    }

    /// Whether this player's marked squares make a line, and how many.
    pub fn lines(&self, player: &Player) -> usize {
        if self.mode != Mode::Bingo {
            return 0;
        }
        let side = self.side as usize;
        if player.card.len() != side * side {
            return 0;
        }
        let on = |r: usize, c: usize| player.marked.contains(&player.card[r * side + c]);
        let rows = (0..side).filter(|&r| (0..side).all(|c| on(r, c))).count();
        let cols = (0..side).filter(|&c| (0..side).all(|r| on(r, c))).count();
        let down = (0..side).all(|i| on(i, i)) as usize;
        let up = (0..side).all(|i| on(i, side - 1 - i)) as usize;
        rows + cols + down + up
    }
}

/// Pairs the room so that people talk to whoever disagrees with them most.
///
/// Every possible pair is scored by the statement they are furthest apart
/// on, preferring statements neither has talked about, and pairs who have
/// already talked come last. The pairs are then taken greedily, biggest
/// disagreement first. With an odd number, the one left over joins the
/// group whose statement they disagree with most. Greedy is not the best
/// possible matching, but it gives the most opposed pairs the evening's
/// best conversations, which is the point.
fn pair(game: &Game, met: &HashSet<(String, String)>) -> (Vec<Group>, Vec<String>) {
    let people: Vec<&Player> = game
        .players
        .iter()
        .filter(|p| !p.answers.is_empty())
        .collect();
    let mut candidates: Vec<(i32, usize, usize, u32)> = Vec::new();
    for i in 0..people.len() {
        for j in i + 1..people.len() {
            let (a, b) = (people[i], people[j]);
            let Some((id, gap)) = game.best_between(a, b) else {
                continue;
            };
            let fresh = !a.marked.contains(&id) && !b.marked.contains(&id);
            let again = met.contains(&(a.key.clone(), b.key.clone()));
            let score = gap as i32 + if fresh { 300 } else { 0 } - if again { 10_000 } else { 0 };
            candidates.push((score, i, j, id));
        }
    }
    // Ties broken by the order people joined, so a round is reproducible.
    candidates.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));

    let mut partner: Vec<Option<usize>> = vec![None; people.len()];
    let mut chosen: Vec<(usize, usize, u32)> = Vec::new();
    for &(_, i, j, id) in &candidates {
        if partner[i].is_some() || partner[j].is_some() {
            continue;
        }
        partner[i] = Some(j);
        partner[j] = Some(i);
        chosen.push((i, j, id));
    }
    // Greedy can strand people a different pairing would seat: A and B
    // take each other, while A could have had C and B had D. Swap
    // partners wherever that pairs two more people, until nothing does.
    let best: std::collections::HashMap<(usize, usize), u32> = candidates
        .iter()
        .map(|&(_, i, j, id)| ((i, j), id))
        .collect();
    let edge = |x: usize, y: usize| best.get(&(x.min(y), x.max(y))).copied();
    loop {
        let mut swapped = false;
        'search: for u in 0..people.len() {
            if partner[u].is_some() {
                continue;
            }
            for k in 0..chosen.len() {
                let (a, b, _) = chosen[k];
                for (x, y) in [(a, b), (b, a)] {
                    let Some(ux) = edge(u, x) else { continue };
                    for v in 0..people.len() {
                        if v == u || partner[v].is_some() {
                            continue;
                        }
                        let Some(vy) = edge(v, y) else { continue };
                        chosen[k] = (u.min(x), u.max(x), ux);
                        chosen.push((v.min(y), v.max(y), vy));
                        partner[u] = Some(x);
                        partner[x] = Some(u);
                        partner[v] = Some(y);
                        partner[y] = Some(v);
                        swapped = true;
                        break 'search;
                    }
                }
            }
        }
        if !swapped {
            break;
        }
    }

    let taken: Vec<bool> = partner.iter().map(|p| p.is_some()).collect();
    let mut groups: Vec<Group> = Vec::new();
    for (i, j, id) in chosen {
        let (a, b) = (people[i], people[j]);
        groups.push(Group {
            members: vec![a.key.clone(), b.key.clone()],
            statement: id,
            before: BTreeMap::from([
                (a.key.clone(), a.answers[&id]),
                (b.key.clone(), b.answers[&id]),
            ]),
            after: BTreeMap::new(),
        });
    }

    let mut waiting = Vec::new();
    for (i, p) in people.iter().enumerate() {
        if taken[i] {
            continue;
        }
        // Into the pair whose statement they are furthest from, where
        // they have answered it; three people still make a conversation.
        let best = groups
            .iter()
            .enumerate()
            .filter(|(_, g)| g.members.len() == 2)
            .filter_map(|(gi, g)| {
                let mine = *p.answers.get(&g.statement)?;
                let s = game.statement(g.statement)?;
                let far = g.before.values().map(|&v| s.gap(mine, v)).max()?;
                Some((far, gi))
            })
            .max();
        match best {
            Some((_, gi)) => {
                let g = &mut groups[gi];
                g.before.insert(p.key.clone(), p.answers[&g.statement]);
                g.members.push(p.key.clone());
            }
            None => waiting.push(p.key.clone()),
        }
    }
    // People who answered nothing wait too, and are told why.
    waiting.extend(
        game.players
            .iter()
            .filter(|p| p.answers.is_empty())
            .map(|p| p.key.clone()),
    );
    (groups, waiting)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(mode: Mode) -> Game {
        let mut g = Game::new(1);
        g.setup(mode, Scale::Written, &["ea".into(), "mind".into()], "", 3)
            .unwrap();
        g
    }

    #[test]
    fn a_bingo_deck_is_about_twice_a_card_and_cards_cover_it_evenly() {
        let mut g = room(Mode::Bingo);
        assert_eq!(g.deck.len(), 18);
        for n in 0..4 {
            g.join(&format!("P{n}"), 1).unwrap();
        }
        let mut used = vec![0; g.deck.len()];
        for p in &g.players {
            assert_eq!(p.card.len(), 9);
            let distinct: BTreeSet<_> = p.card.iter().collect();
            assert_eq!(distinct.len(), 9, "no square twice");
            for &id in &p.card {
                used[id as usize] += 1;
            }
        }
        assert!(used.iter().all(|&n| n == 2), "{used:?}");
    }

    #[test]
    fn the_open_mode_takes_every_statement() {
        let g = room(Mode::Open);
        let all = topic("ea").unwrap().statements.len() + topic("mind").unwrap().statements.len();
        assert_eq!(g.deck.len(), all);
        assert!(
            g.deck.iter().any(|s| s.chance),
            "probability questions are in"
        );
    }

    #[test]
    fn custom_lines_join_the_deck_and_a_percent_sign_asks_for_a_chance() {
        let mut g = Game::new(1);
        g.setup(
            Mode::Open,
            Scale::Written,
            &[],
            "Cats are better than dogs.\n%Will it rain tomorrow?\n\n",
            4,
        )
        .unwrap();
        assert_eq!(g.deck.len(), 2);
        assert!(!g.deck[0].chance);
        assert!(g.deck[1].chance);
        assert_eq!(g.deck[1].text, "Will it rain tomorrow?");
        assert!(
            g.setup(Mode::Bingo, Scale::Written, &[], "only one", 3)
                .is_err(),
            "too few for a card"
        );
    }

    #[test]
    fn answers_stay_on_the_scale_and_the_first_one_is_kept() {
        let mut g = room(Mode::Open);
        let k = g.join("Ada", 1).unwrap();
        let chance = g.deck.iter().find(|s| s.chance).unwrap().id;
        let agree = g.deck.iter().find(|s| !s.chance).unwrap().id;
        assert!(g.answer(&k, chance, Some(-5)).is_err());
        assert!(g.answer(&k, agree, Some(101)).is_err());
        g.answer(&k, agree, Some(-60)).unwrap();
        g.answer(&k, agree, Some(20)).unwrap();
        let p = g.player(&k).unwrap();
        assert_eq!(p.first[&agree], -60);
        assert_eq!(p.answers[&agree], 20);
        assert!(
            g.setup(Mode::Bingo, Scale::Written, &["ea".into()], "", 3)
                .is_err(),
            "locked once answered"
        );
    }

    #[test]
    fn a_round_pairs_the_most_opposed_and_the_odd_one_makes_three() {
        let mut g = room(Mode::Open);
        let s = g.deck.iter().find(|s| !s.chance).unwrap().id;
        let t = g.deck.iter().filter(|s| !s.chance).nth(1).unwrap().id;
        let keys: Vec<String> = ["A", "B", "C", "D", "E"]
            .iter()
            .map(|n| g.join(n, 1).unwrap())
            .collect();
        // A and D are opposite on s; B and C on t; E agrees with A on s.
        for (k, (vs, vt)) in keys
            .iter()
            .zip([(100, 0), (0, 90), (0, -90), (-100, 0), (90, 0)])
        {
            g.answer(k, s, Some(vs)).unwrap();
            g.answer(k, t, Some(vt)).unwrap();
        }
        g.start_round(1000).unwrap();
        let r = g.rounds.last().unwrap();
        assert_eq!(r.groups.len(), 2);
        let ad = r
            .groups
            .iter()
            .find(|g| g.members.contains(&keys[0]))
            .unwrap();
        assert!(ad.members.contains(&keys[3]), "A talks to D");
        assert_eq!(ad.statement, s);
        let bc = r
            .groups
            .iter()
            .find(|g| g.members.contains(&keys[1]))
            .unwrap();
        assert!(bc.members.contains(&keys[2]), "B talks to C");
        assert_eq!(bc.statement, t);
        assert_eq!(ad.members.len() + bc.members.len(), 5, "E joins a group");
        assert!(r.waiting.is_empty());
        assert_eq!(g.phase, Phase::Talking);
        assert_eq!(g.ends, 1000 + 5 * 60_000);
        assert!(g.player(&keys[0]).unwrap().marked.contains(&s));
    }

    #[test]
    fn the_clock_ends_a_round_and_the_new_answer_is_recorded() {
        use rooms::Room;
        let mut g = room(Mode::Open);
        let s = g.deck.iter().find(|s| !s.chance).unwrap().id;
        let a = g.join("A", 1).unwrap();
        let b = g.join("B", 1).unwrap();
        g.answer(&a, s, Some(80)).unwrap();
        g.answer(&b, s, Some(-80)).unwrap();
        g.start_round(0).unwrap();
        assert!(!g.tick(1000));
        assert!(g.tick(g.ends));
        assert_eq!(g.phase, Phase::Updating);
        g.answer(&a, s, Some(30)).unwrap();
        let group = &g.rounds[0].groups[0];
        assert_eq!(group.before[&a], 80);
        assert_eq!(group.after[&a], 30);
    }

    #[test]
    fn people_who_have_talked_are_not_paired_again_while_others_are_free() {
        let mut g = room(Mode::Open);
        let s = g.deck.iter().find(|s| !s.chance).unwrap().id;
        let keys: Vec<String> = ["A", "B", "C", "D"]
            .iter()
            .map(|n| g.join(n, 1).unwrap())
            .collect();
        for (k, v) in keys.iter().zip([100, -100, 50, -50]) {
            g.answer(k, s, Some(v)).unwrap();
        }
        g.start_round(0).unwrap();
        let first: Vec<Vec<String>> = g.rounds[0]
            .groups
            .iter()
            .map(|g| g.members.clone())
            .collect();
        g.stop().unwrap();
        g.start_round(1).unwrap();
        for group in &g.rounds[1].groups {
            assert!(
                !first.contains(&group.members),
                "{:?} met twice",
                group.members
            );
        }
    }

    #[test]
    fn nobody_is_stranded_when_a_different_pairing_seats_everyone() {
        let mut g = room(Mode::Open);
        let ids: Vec<u32> = g
            .deck
            .iter()
            .filter(|s| !s.chance)
            .take(3)
            .map(|s| s.id)
            .collect();
        let keys: Vec<String> = ["A", "B", "C", "D"]
            .iter()
            .map(|n| g.join(n, 1).unwrap())
            .collect();
        // A and B are furthest apart, on s0; C shares s1 only with A, and
        // D shares s2 only with B. Greedy alone would leave C and D out.
        g.answer(&keys[0], ids[0], Some(100)).unwrap();
        g.answer(&keys[1], ids[0], Some(-100)).unwrap();
        g.answer(&keys[0], ids[1], Some(0)).unwrap();
        g.answer(&keys[2], ids[1], Some(40)).unwrap();
        g.answer(&keys[1], ids[2], Some(0)).unwrap();
        g.answer(&keys[3], ids[2], Some(40)).unwrap();
        g.start_round(0).unwrap();
        let r = &g.rounds[0];
        assert!(r.waiting.is_empty(), "{:?}", r.waiting);
        assert_eq!(r.groups.len(), 2);
    }

    #[test]
    fn moving_towards_your_partner_scores_for_them_not_you() {
        let mut g = room(Mode::Open);
        let s = g.deck.iter().find(|s| !s.chance).unwrap().id;
        let a = g.join("A", 1).unwrap();
        let b = g.join("B", 1).unwrap();
        g.answer(&a, s, Some(80)).unwrap();
        g.answer(&b, s, Some(-80)).unwrap();
        g.start_round(0).unwrap();
        g.stop().unwrap();
        // B comes 60 of the 160 points towards A; A moves away from B.
        g.answer(&b, s, Some(-20)).unwrap();
        g.answer(&a, s, Some(100)).unwrap();
        let p = g.persuasion();
        assert_eq!(p[&a].points, 60);
        assert_eq!(p[&a].people, 1);
        assert!(!p.contains_key(&b), "moving away gives the partner nothing");
    }

    #[test]
    fn a_full_row_of_marks_is_bingo() {
        let mut g = room(Mode::Bingo);
        let k = g.join("A", 1).unwrap();
        let card = g.player(&k).unwrap().card.clone();
        for &id in &card[..2] {
            g.mark(&k, id, true).unwrap();
        }
        assert_eq!(g.lines(g.player(&k).unwrap()), 0);
        g.mark(&k, card[2], true).unwrap();
        assert_eq!(g.lines(g.player(&k).unwrap()), 1);
        g.mark(&k, card[4], true).unwrap();
        g.mark(&k, card[8], true).unwrap();
        assert_eq!(g.lines(g.player(&k).unwrap()), 2, "and the diagonal");
    }

    #[test]
    fn the_room_can_answer_everything_one_way() {
        let mut g = Game::new(1);
        g.setup(
            Mode::Open,
            Scale::Agree,
            &["mind".into()],
            "%What is the chance that it snows?",
            4,
        )
        .unwrap();
        assert!(g.deck.iter().all(|s| !s.chance));
        assert!(
            g.deck
                .iter()
                .any(|s| s.text == "Today's large language models are conscious.")
        );
        assert!(g.deck.iter().any(|s| s.text == "It snows."));
        g.setup(Mode::Open, Scale::Chance, &["mind".into()], "", 4)
            .unwrap();
        assert!(g.deck.iter().all(|s| s.chance));
        assert!(g.deck.iter().any(|s| s.text == "Free will is an illusion."));
    }

    #[test]
    fn every_probability_question_in_the_bank_becomes_a_claim() {
        for t in TOPICS {
            for q in t.statements.iter().filter_map(|s| s.strip_prefix("% ")) {
                let c = claim(q);
                assert!(!c.starts_with("What") && c.ends_with('.'), "{q} -> {c}");
            }
        }
    }

    #[test]
    fn a_probability_gap_counts_double() {
        let s = Statement {
            id: 0,
            text: String::new(),
            topic: String::new(),
            chance: true,
        };
        assert_eq!(s.gap(10, 60), 100);
    }
}
