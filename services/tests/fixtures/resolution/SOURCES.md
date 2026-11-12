# Recorded source responses

Retrieved 7 November 2026 for the resolver tests (G5 unit 5). Terms: `docs/data-licenses.md`.

| File | Request |
|---|---|
| `bls_cpi_2025_2026.json` | `https://api.bls.gov/publicAPI/v1/timeseries/data/CUUR0000SA0?startyear=2025&endyear=2026`. Source: U.S. Bureau of Labor Statistics. BLS.gov cannot vouch for the data or analyses derived from these data after the data have been retrieved from BLS.gov. October 2025 is published as `-` (no data, because of the 2025 lapse in appropriations). |
| `noaa_tmax_2025-07-01_03.json` | `https://www.ncei.noaa.gov/access/services/data/v1?dataset=daily-summaries&stations=USW00094728&startDate=2025-07-01&endDate=2025-07-03&dataTypes=TMAX&units=metric&format=json`. Source: NOAA National Centers for Environmental Information, GHCN-Daily (Menne et al. 2012). |
| `noaa_tmax_not_published.json` | The same request for 2026-11-07, the day of retrieval: no record yet. |

| `nws_cli_den_2026-11-06.json` | Retrieved 8 November 2026. `https://api.weather.gov/products?type=CLI&location=DEN&start=2026-11-06T00:00:00Z&end=...` for two end times (before and after the final report was issued), and every report those searches list (`https://api.weather.gov/products/{id}`). Source: National Weather Service, Boulder office (a U.S. government work). |
| `usgs_m5_2026-11-05.json`, `usgs_m6_2026-11-05.json` | Retrieved 8 November 2026. `https://earthquake.usgs.gov/fdsnws/event/1/query?format=geojson&eventtype=earthquake&starttime=2026-11-05T00:00:00&endtime=2026-11-05T23:59:59.999&minmagnitude=5` (and `=6`) `&orderby=time-asc&limit=1000`. Source: U.S. Geological Survey, ANSS Comprehensive Catalog (public domain). |
| `swpc_kp_2026-11-08.json` | Retrieved 8 November 2026. `https://services.swpc.noaa.gov/products/noaa-planetary-k-index.json`: 3-hour estimated Kp from 1 November to the morning of 8 November. Source: NOAA Space Weather Prediction Center (public domain). |

FRED responses are not recorded here (the Bank's terms; `docs/data-licenses.md`). The FRED tests use responses written by hand in FRED's documented format.

## Sports (G10)
Recorded on 9 November 2026 with the owner's free keys (sent in a header, never stored here).
- `balldontlie_nba_21718052.json`: `GET https://api.balldontlie.io/v1/games/21718052`, a final NBA game of 7 November 2026 (home 102, visitors 114).
- `balldontlie_nfl_1392335.json`: `GET https://api.balldontlie.io/nfl/v1/games/1392335`, a final NFL game of 1 November 2026 local time (home 27, visitors 10).
- `football_data_560631.json`: `GET https://api.football-data.org/v4/matches/560631`, Liverpool 0 Arsenal 1, Premier League, 1 November 2026.
