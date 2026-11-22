---
name: research/web-search
description: Load when the answer is not in this repository — fetching a page whose address you know, and running a general search when you do not, including what to do when the search engine refuses an automated client. Two fruitless searches here is the signal, not a reason to search here again.
---

# Looking outside this repository

## A page whose address you already have

`fetch` retrieves it. HTML comes back as Markdown, so the result is prose
rather than markup; pass `raw: true` when the markup itself is what you need. A
URL that resolves to an image comes back as an image, so a screenshot or a
diagram can be looked at rather than described.

Documentation, changelogs, release notes and RFCs are one fetch away once you
know the address. Reach for this first — it is exact, and it costs one call.

## A general search

When you cannot name the page, fetch DuckDuckGo's no-JavaScript results page:

```text
fetch(
  url: "https://duckduckgo.com/html/?q=<your query, url-encoded>"
)
```

The result is a list of titles and links in Markdown. **It is an index, not an
answer.** Read it to decide which page to fetch, then fetch that page for the
content. Quoting a search result's one-line summary as fact is how wrong answers
get written — the summary is written to attract a click, not to be correct.

A web search engine matches keywords; the search server here matches meaning. Two
or three terms for the first, a whole question for the second.

This is a GET with the query in the URL, not a POST with a `body`. Measured, and
the difference is not cosmetic: the POST form to the `lite.` host answered with a
bot check every time it was tried, while the URL form above answered with results.
A search is a GET, and this endpoint wants it that way.

## When the search is refused

**A successful call is not an answer.** DuckDuckGo answers an automated client
with a bot check instead of results, and it does so **intermittently** — the same
call that returned results a minute ago comes back with the check. This is a
property of the service, not of your query, and nothing you write in a query
changes it.

**The refusal is not an error.** The page arrives with a `200` and a normal
`content-type`; what it contains is a challenge ("Select all squares containing a
duck") or the site's front page. So the signal is **a result with no list of links
in it**, not a failed call. A search that returns a page you cannot read a link out
of has been refused, and reporting those contents as an answer is the failure this
section exists to prevent.

When that happens, stop — and ask a person:

- **Do not retry it.** The refusal is not transient within a run.
- **Do not switch endpoint or add a header** to look like something else. Getting
  past the check is a workaround, it breaks the next time the service changes, and
  it is not what the service asks of automated clients.
- **Say so in your report**, with the query that would answer it: "a web search is
  needed for X". End without calling anyone, and the machinery tells the person
  this work belongs to. A person runs it in seconds, and the alternative is this
  run spending its budget on a wall.

**Do not read a refusal as "the answer does not exist".** That is the mistake this
section exists to prevent. A page whose address you know is still one `fetch`
away, and the repository's own history is still worth searching — see below.

## Judgement

Search the repository's own history before searching the web.
`search_issues` answers "why is it like this" better than any external
page, because the reason was written down here at the time it was decided — and
usually in a comment, which is why that tool reports which comment it matched.
Read that comment with `get_issue_comments` before reaching outward.

Say where a claim came from. Something read off a web page is worth less than
something read out of the code, and whoever reads your report needs to know
which one they are looking at.

## Replacing this

The search endpoint above is in this skill rather than inside the tool, on
purpose. To use a different service — one with an API key, or your own — edit
this file; the tool fetches whatever URL it is handed, so nothing else changes.

Pick the replacement knowing what was measured here: every large engine's
`robots.txt` forbids its results path (`Disallow: /search` on Bing, Mojeek and
Brave; DuckDuckGo's own front page answers a plain client with a JavaScript shell
and redirects to the same no-JavaScript page this section names), and the
community search front-ends that allow a scripted client at all rate-limit one
shared IP. A keyed API is the one form that is both permitted and reliable, at the
cost of a key to configure.

If you would rather agents did not query a public search engine at all, delete
this section. Fetching a named page keeps working.
