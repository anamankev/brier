// The photo each event shows, and who took it. Every photo comes from Wikimedia Commons under a
// licence that allows reuse: public domain, CC0, or Creative Commons Attribution (some
// ShareAlike). Each is cropped to a square and resized; the files are in public/photos and the
// credits are listed on /credits.

export type Photo = {
  /** What the photo shows, for the credits page. */
  subject: string;
  author: string;
  license: "Public domain" | "CC0" | "CC BY 2.0" | "CC BY-SA 3.0" | "CC BY-SA 4.0";
  /** The photo's page on Wikimedia Commons, with the original file and its licence. */
  source: string;
};

const commons = (file: string) => `https://commons.wikimedia.org/wiki/File:${file}`;

export const PHOTOS = {
  nyc: { subject: "Midtown Manhattan from Top of the Rock", author: "Nan Palmero", license: "CC BY 2.0", source: commons("Empire_State_Building_at_Night_from_Top_of_the_Rock_(26453941607).jpg") },
  "central-park": { subject: "Central Park in the January 2015 blizzard", author: "Anthony Quintano", license: "CC BY 2.0", source: commons("Blizzard_of_2015_in_Central_Park_New_York_City_-_Flickr_-_Anthony_Quintano.jpg") },
  chicago: { subject: "Chicago skyline and lakefront", author: "Carol M. Highsmith", license: "Public domain", source: commons("Aerial_view_of_skyline,_Chicago,_Illinois_LCCN2011632330.tif") },
  "los-angeles": { subject: "Downtown Los Angeles and the San Gabriel Mountains", author: "Carol M. Highsmith", license: "Public domain", source: commons("Skyline_view_of_Los_Angeles,_California_LCCN2013631685.tif") },
  miami: { subject: "Downtown Miami from Biscayne Bay", author: "Carol M. Highsmith", license: "Public domain", source: commons("Miami,_Florida_LCCN2011630184.tif") },
  boston: { subject: "Boston from the Charles River", author: "Carol M. Highsmith", license: "Public domain", source: commons("St._Charles_River_view_of_Boston,_Massachusetts_LCCN2011631157.tif") },
  seattle: { subject: "Seattle at dusk, with Mount Rainier", author: "Carol M. Highsmith", license: "Public domain", source: commons("Dusk_view_of_the_skyline,_Seattle,_Washington_LCCN2011630307.tif") },
  denver: { subject: "Denver skyline and the Front Range", author: "Carol M. Highsmith", license: "Public domain", source: commons("Skyline,_Denver,_Colorado_LCCN2011636031.tif") },
  atlanta: { subject: "Midtown Atlanta at dusk", author: "ericstokley", license: "CC0", source: commons("Midtown_atlanta.jpg") },
  phoenix: { subject: "Downtown Phoenix from the air", author: "DPPed", license: "CC BY-SA 3.0", source: commons("Downtown_Phoenix_Aerial_Looking_Northeast.jpg") },
  "washington-dc": { subject: "Lincoln Memorial, Washington Monument and Capitol at dusk", author: "Carol M. Highsmith", license: "Public domain", source: commons("Dusk_view_of_the_Lincoln_Memorial,_Washington_Monument_and_U.S._Capitol,_Washington,_D.C_LCCN2011631138.tif") },
  austin: { subject: "Austin skyline over Lady Bird Lake", author: "Michael Barera", license: "CC BY-SA 4.0", source: commons("Austin_August_2019_19_(skyline_and_Lady_Bird_Lake).jpg") },
  dallas: { subject: "Dallas skyline at night", author: "Carol M. Highsmith", license: "Public domain", source: commons("Dallas_night_skyline.jpg") },
  weather: { subject: "Supercell thunderstorm in Kansas", author: "NOAA National Severe Storms Laboratory", license: "Public domain", source: commons("Supercell-thunderstorm-in-Kansas.jpg") },
  earthquakes: { subject: "A seismograph drum at Weston Observatory", author: "Z22", license: "CC BY-SA 3.0", source: commons("Seismogram_at_Weston_Observatory.JPG") },
  "space-weather": { subject: "Aurora seen from the International Space Station", author: "NASA", license: "Public domain", source: commons("Aurora_Seen_From_Space_by_NASA.jpg") },
  cpi: { subject: "A grocery cart", author: "U.S. Department of Agriculture", license: "Public domain", source: commons("20250219-USDA-FNS-UNK-0006_(54498997849).jpg") },
  jobs: { subject: "A hiring fair", author: "U.S. Space Force (Van Ha)", license: "Public domain", source: commons("Air_Force_Civilian_Service_Hiring_Fair_(6107214).jpg") },
  fed: { subject: "The Federal Reserve's Eccles Building", author: "Federal Reserve", license: "Public domain", source: commons("Eccles_Building_(26088200676).jpg") },
  treasury: { subject: "The Treasury Building, Washington", author: "Sealy j", license: "CC BY-SA 4.0", source: commons("U.S._Treasury_Building_and_Albert_Gallatin_Statue.jpg") },
  basketball: { subject: "A dunk under the arena lights", author: "U.S. Navy (Mass Communication Specialist 2nd Class James R. Evans)", license: "Public domain", source: commons("US_Navy_111110-N-DR144-460_University_of_North_Carolina_basketball_player_Tyler_Zeller_dunks_during_a_practice_in_the_basketball_arena_on_the_fligh.jpg") },
  football: { subject: "A ball carrier breaks a tackle", author: "Damon J. Moritz, U.S. Navy", license: "Public domain", source: commons("2005_Temple-Navy_Game_02.jpg") },
  soccer: { subject: "A floodlit soccer stadium at night", author: "YoTuT", license: "CC BY 2.0", source: commons("LA_Galaxy_vs_Houston_Dynamo-_Western_Conference_Finals_panorama.jpg") },
} as const satisfies Record<string, Photo>;

export type PhotoKey = keyof typeof PHOTOS;

export const LICENSE_URLS: Record<Photo["license"], string | null> = {
  "Public domain": null,
  CC0: "https://creativecommons.org/publicdomain/zero/1.0/",
  "CC BY 2.0": "https://creativecommons.org/licenses/by/2.0/",
  "CC BY-SA 3.0": "https://creativecommons.org/licenses/by-sa/3.0/",
  "CC BY-SA 4.0": "https://creativecommons.org/licenses/by-sa/4.0/",
};

// Words in a title that pick a photo, checked in order: Central Park before New York, the
// specific economic series before the category.
const BY_TITLE: [RegExp, PhotoKey][] = [
  [/central park/i, "central-park"],
  [/new york|\bnyc\b/i, "nyc"],
  [/chicago/i, "chicago"],
  [/los angeles/i, "los-angeles"],
  [/miami/i, "miami"],
  [/boston/i, "boston"],
  [/seattle/i, "seattle"],
  [/denver/i, "denver"],
  [/atlanta/i, "atlanta"],
  [/phoenix/i, "phoenix"],
  [/washington/i, "washington-dc"],
  [/austin/i, "austin"],
  [/dallas/i, "dallas"],
  [/earthquake|quake|magnitude/i, "earthquakes"],
  [/geomagnetic|aurora|solar/i, "space-weather"],
  [/\bcpi\b|inflation|prices/i, "cpi"],
  [/unemployment|payrolls|jobs/i, "jobs"],
  [/treasury|yield/i, "treasury"],
  [/\bfed\b|federal funds|target rate/i, "fed"],
];

const BY_CATEGORY: Record<string, PhotoKey> = { weather: "weather", science: "earthquakes", economics: "cpi", rates: "fed", sports: "soccer" };
const LEAGUE_SPORT: Record<string, PhotoKey> = {
  nba: "basketball",
  nfl: "football",
  "premier-league": "soccer",
  "la-liga": "soccer",
  "serie-a": "soccer",
  bundesliga: "soccer",
  "ligue-1": "soccer",
};

/** The photo for an event, market or topic: its tags first (a city, earthquakes, space
 * weather), then words in its title, then its category. */
export function photoFor(x: { title?: string | null; category?: string | null; tags?: string[] }): PhotoKey {
  for (const t of x.tags ?? []) if (t in PHOTOS) return t as PhotoKey;
  for (const t of x.tags ?? []) if (LEAGUE_SPORT[t]) return LEAGUE_SPORT[t]!;
  for (const [re, key] of BY_TITLE) if (x.title && re.test(x.title)) return key;
  return BY_CATEGORY[x.category ?? ""] ?? "cpi";
}
