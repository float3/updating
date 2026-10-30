# updating

[![CI](https://github.com/float3/updating/actions/workflows/ci.yml/badge.svg)](https://github.com/float3/updating/actions/workflows/ci.yml)

speed updating for a room of people, at
[updating.rationality-munich.com](https://updating.rationality-munich.com).

everyone says where they stand on a set of statements, then talks to
whoever disagrees with them most, a few minutes at a time, and says where
they stand afterwards. the format comes from EA groups, who run it on
paper; thanks to Noah Birnbaum for telling us about it.

built on [rooms](https://github.com/float3/rooms).

## how an evening goes

the facilitator opens a room, picks topics and puts the join link on the
wall. everyone scans it and gives a name, then answers statements:

| | |
| --- | --- |
| a statement | how much you agree, from −100 to 100 |
| a question asking for a probability | the chance you give it, from 0 to 100% |

most statements can be put either way, so the facilitator chooses: as
written (mostly agreement, some probabilities), all agreement, where a
question like "What is the chance that a fish feels pain?" becomes the
claim "A fish feels pain.", or all probabilities, where every statement
is answered with the chance that it is true.

in the **bingo** mode each person is dealt a grid of statements, 3×3 to
5×5. the statements they talk about are marked, and a full line is a
bingo. in the **open** mode everybody sees every statement from the chosen
topics and answers as many as they like.

the **free-for-all** mode is for browsing on your own time, over an
evening or longer. every statement shows how many people have answered
it, and the most answered come first, so a room with a large deck
converges on the statements people care about. you see who answered a
statement, and what they said, only after answering it yourself: knowing
it first would anchor your answer. each phone keeps pointing its owner at
whoever disagrees with them most ("Josh, go and talk to Jake"), and the
facilitator can still run timed rounds when the room wants them.

the facilitator starts a round. the server pairs everybody with the person
they disagree with most, on the statement they are furthest apart on, and
a clock runs: two to ten minutes. when it runs out, each phone asks how
much the conversation changed their mind, with the slider where they left
it. then the next round, with somebody new.

between rounds each phone also lists the people who disagree with it
most, and on what, so people can go and find each other by themselves.

## pairing

every possible pair is scored by the statement they are furthest apart on,
preferring statements neither has talked about yet, and pairs who have
already talked come last. pairs are taken greedily, biggest disagreement
first, then repaired: wherever swapping partners seats two more people, the
swap is made. with an odd number, the one left over joins the pair whose
statement they are furthest from, as a three. people who have answered
nothing anybody else has answered are told they were not paired, and why.

a probability's scale is half as wide as agreement's, so its gaps count
double when the two are compared.

## who wins

the person who moved others most. after a round, every point a partner
moves towards your answer on the statement you were given is a point for
you. moving away earns the partner nothing, and your own movement never
scores for you, so the only way to score is to change somebody else's
mind. the facilitator sees the board all evening; everybody sees the top
three when the evening ends.

the facilitator also sees where the room is most split, what people
changed their minds on, and how many answers each person gave and changed.

## statements

167 in `src/statements.rs`, by topic: effective altruism, AI
safety, animals and veganism, metaethics, ethics, consciousness and mind,
metaphysics, knowledge and rationality, society and politics, the future,
and a warm-up set. a bingo room samples about twice a card's worth, so
that cards overlap enough for people to have statements in common.

the facilitator can add their own, one per line; a line starting with `%`
asks for a probability.

anybody can suggest a statement from the start page or from their phone.
suggestions are appended to a file on the server, one JSON line each, and
nothing serves them back:

```sh
sudo cat /var/lib/private/updating/suggestions.jsonl
```

## three links

| | |
| --- | --- |
| `/h/<key>` | runs the room. Keep it; it is the only way back in. |
| `/j/<key>` | joins it. This is the one on the wall. |
| `/p/<key>` | one player's own, kept by their phone. |

the key in the link is the only access control: 24 random characters and
no accounts. see [rooms](https://github.com/float3/rooms) for what that
does and does not protect.

the facilitator sees everything. a player is sent their own answers, and
once paired, where their partners stand on the statement they were given:
that is the conversation. between rounds they are sent the five people
who disagree with them most, with both answers on that one statement.

## running it

```sh
nix run github:float3/updating
```

| | |
| --- | --- |
| `UPDATING_ADDR` | what to listen on. `127.0.0.1:8102`. |
| `UPDATING_STATE` | where the rooms are kept. `rooms.json`. |
| `UPDATING_SUGGESTIONS` | where suggested statements go. `suggestions.jsonl`. |

the NixOS module takes a port and the domains to serve on:

```nix
services.updating = {
  enable = true;
  port = 8102;
  domains = ["updating.example.com"];
};
```

rooms are forgotten a day after anyone last looked at one.

## development

```sh
nix develop --command cargo test
```

the page is TypeScript in `client/`, compiled into the binary by the build
script with [rooms-client](https://github.com/float3/rooms). no
JavaScript is written by hand here.
