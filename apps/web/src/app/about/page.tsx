import type { Metadata } from "next";
import { AppShell } from "../../components/shell";
import "./about.css";

export const metadata: Metadata = { title: "About · SceneCask", referrer: "no-referrer" };

/** D04 About: TMDB attribution and the metadata availability disclaimer. Public, no account data. */
export default function AboutPage() {
  return (
    <AppShell currentPath="/about" homeHref="/home">
      <article className="sc-reading-column sc-about" aria-labelledby="sc-about-title">
        <h1 id="sc-about-title">About SceneCask</h1>
        <p className="sc-reading">
          SceneCask keeps track of the TV episodes you’ve watched, so episode titles, stills and summaries stay hidden
          until you’ve seen them. It doesn’t stream video.
        </p>
        <section className="sc-about-section" aria-labelledby="sc-about-tmdb">
          <h2 id="sc-about-tmdb">TV metadata</h2>
          <p className="sc-reading">
            This product uses the TMDB API but is not endorsed or certified by TMDB. Show and episode information
            and artwork come from{" "}
            <a href="https://www.themoviedb.org/" rel="noreferrer">
              The Movie Database (TMDB)
            </a>
            .
          </p>
          <p className="sc-reading">
            Listings can be incomplete or change after you save a show. Release dates without a reliable time zone use
            midnight UTC, so the time an episode becomes available to you may differ. SceneCask keeps the last
            information it received when TMDB is unavailable, and your watch history is never removed because
            listings changed.
          </p>
        </section>
      </article>
    </AppShell>
  );
}
