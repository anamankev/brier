# Data sources and their terms (SPEC §7, O3)

Read on 7 November 2026. Every resolution source must be free, public, and authoritative, and its terms must allow how Brier uses it: fetching values to settle play-money markets, keeping the raw responses as evidence, and showing the values to users. This file records what each source's terms say and what Brier does about it. It is a reading of public terms, not legal advice; re-check before any public deployment.

## Bureau of Labor Statistics (BLS Public Data API)
- **Status:** BLS publications and data are in the public domain and may be used without permission (bls.gov/opub/copyright-information.htm). BLS asks to be cited as the source.
- **API terms** (bls.gov/developers/termsOfService.htm):
  - cite the date the data were retrieved;
  - state that BLS.gov cannot vouch for the data or for analyses derived from them;
  - never modify content while citing BLS as the source;
  - do not use the BLS logo.
- **Limits:** version 1 needs no key, and allows 25 queries a day, 25 series per query and 10 years per query. It returns no percent changes; Brier computes them itself.
- **Brier's use:** CPI-U, U.S. city average, all items, not seasonally adjusted (`CUUR0000SA0`), and the unemployment rate (`LNS14000000`). Raw responses **may be committed** as test fixtures. Evidence pages show the citation, the retrieval date, and the BLS disclaimer.

## NOAA National Centers for Environmental Information (Access Data Service, GHCN-Daily)
- **Status:** a work of the U.S. federal government, so it carries no U.S. copyright (17 U.S.C. §105). The dataset record names no license. Its use constraints:
  - NOAA and NCEI make no warranty;
  - users decide whether the data fit their purpose;
  - electronic downloads are free.
- **Citation required:** Menne et al. (2012), *Global Historical Climatology Network - Daily (GHCN-Daily), Version 3*, NOAA NCDC, doi:10.7289/V5D21VHZ, with the subset used and the access date. Publications should also cite Menne et al. (2012), *J. Atmos. Oceanic Technol.* 29, 897-910.
- **Access:** `https://www.ncei.noaa.gov/access/services/data/v1`, HTTPS only, no token (checked: a three-day TMAX query for `USW00094728` returned JSON).
- **Brier's use:** daily maximum temperature at named stations. Raw responses **may be committed**. Evidence pages show the citation and the no-warranty statement.

## Federal Reserve Bank of St. Louis (FRED and ALFRED API)
- **Terms** (fred.stlouisfed.org/docs/api/terms_of_use.html):
  - an API key, tied to the person who registered it, who is responsible for its use;
  - no replicating FRED's own user experience;
  - the Bank claims rights in the API's output;
  - series owned by third parties need the owner's permission for anything beyond personal use, which the Bank cannot grant;
  - an application must state that it "uses the FRED® API but is not endorsed or certified by the Federal Reserve Bank of St. Louis";
  - an application must link to the terms and say that users are bound by them.
- **Brier's use:** only series whose underlying source is public domain:
  - the federal funds target range upper limit (`DFEDTARU`, from FOMC statements);
  - the 10-year Treasury constant-maturity yield (`DGS10`, from the Board's H.15 release).
  - The Board of Governors' material is in the public domain and may be copied with credit to the Board (federalreserve.gov/disclaimer.htm).
- **Not used: the S&P 500 (`SP500`).** FRED's notes say the series is provided under an agreement with S&P Dow Jones Indices; it is a third-party series, and Brier has no permission from the owner. The S&P 500 launch market is replaced by a 10-year Treasury yield market (SPEC §7, G5 plan).
- **Raw FRED responses are not committed**, because the Bank claims rights in output.
  - CI uses fixtures written by hand in FRED's response format.
  - Real responses stay in the local evidence store (`var/evidence/`, git-ignored).
  - The exit test runs on real recorded responses locally.
- **The key** lives only in `.env` (git-ignored, mode 600). It is stripped from every evidence record and log line.
- **The web app (G6)** shows the FRED notice and a link to the terms wherever FRED data appear.

## Consequences for the rest of the spec
- **O6 (new):** markets on licensed indexes such as the S&P 500 need a source whose terms allow this use. Until one is found:
  - there are no index markets;
  - the G7 market maker's options-implied fair value (SPEC §8) has no market to quote, and G7 decides between finding a licensed source and quoting the remaining markets with a documented simple model.
