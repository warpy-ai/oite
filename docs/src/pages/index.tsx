import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';

import styles from './index.module.css';

export default function Home(): ReactNode {
  const {siteConfig} = useDocusaurusContext();
  return (
    <Layout
      title="Script - High Performance Systems Language"
      description="Script is a high-performance systems language with TypeScript syntax, compiling to native code via Cranelift JIT and LLVM AOT. Features self-hosting compiler, ownership model, and SSA IR optimizations.">
      <main className={clsx(styles.page)}>
        <section className={clsx(styles.heroShell)}>
          <div className="container">
            <div className={styles.heroInner}>
              <div>
                <div className={styles.eyebrow}>Script Language</div>
                <h1 className={styles.title}>
                  TypeScript syntax.
                  <br />
                  Native performance.
                </h1>
                <p className={styles.subtitle}>
                  Script is a high‑performance systems language with TypeScript syntax,
                  compiling to native code via Cranelift JIT and LLVM AOT. Features a
                  self‑hosting compiler, Rust‑inspired ownership model, and SSA IR optimizations.
                </p>
                <div className={styles.heroActions}>
                  <Link
                    className={clsx('button button--primary button--lg', styles.primaryCta)}
                    to="/docs/intro">
                    Get started with the docs
                  </Link>
                  <Link
                    className={clsx('button button--secondary button--lg', styles.secondaryCta)}
                    to="/blog">
                    Read the latest updates
                  </Link>
                </div>
                <p className={styles.heroMeta}>
                  Self‑hosting complete • 113 tests passing • ~30x faster native code
                </p>
              </div>
              <aside className={styles.heroAside}>
                <div className={styles.heroAsideTitle}>What's inside</div>
                <div className={styles.heroAsideGrid}>
                  <div className={styles.heroAsideCard}>
                    <div className={styles.heroAsideCardTitle}>SSA IR</div>
                    <div className={styles.heroAsideCardBody}>
                      Register‑based SSA IR with type inference, constant folding, and DCE.
                    </div>
                  </div>
                  <div className={styles.heroAsideCard}>
                    <div className={styles.heroAsideCardTitle}>Self‑Hosting</div>
                    <div className={styles.heroAsideCardBody}>
                      Compiler written in Script itself, generating LLVM IR for native binaries.
                    </div>
                  </div>
                  <div className={styles.heroAsideCard}>
                    <div className={styles.heroAsideCardTitle}>Native Backend</div>
                    <div className={styles.heroAsideCardBody}>
                      Cranelift JIT for development, LLVM AOT with ThinLTO/Full LTO for production.
                    </div>
                  </div>
                  <div className={styles.heroAsideCard}>
                    <div className={styles.heroAsideCardTitle}>Ownership</div>
                    <div className={styles.heroAsideCardBody}>
                      Rust‑inspired ownership model with compile‑time borrow checking.
                    </div>
                  </div>
                </div>
              </aside>
            </div>
          </div>
        </section>

        <section className={styles.sectionShell}>
          <div className="container">
            <div className={styles.sectionHeader}>
              <div className={styles.sectionTitle}>Why Script</div>
              <p className={styles.sectionSubtitle}>
                A minimal core language (like C without libc) with optional Rolls ecosystem libraries.
              </p>
            </div>

            <div className={styles.featureGrid}>
              <article className={styles.featureCard}>
                <div className={styles.featureLabel}>Performance</div>
                <h3 className={styles.featureTitle}>Native code speed</h3>
                <p className={styles.featureBody}>
                  Compiles to native binaries via LLVM with LTO. Native code runs ~30x faster than VM, JIT ~6x faster.
                </p>
              </article>
              <article className={styles.featureCard}>
                <div className={styles.featureLabel}>Syntax</div>
                <h3 className={styles.featureTitle}>TypeScript‑like</h3>
                <p className={styles.featureBody}>
                  Familiar JavaScript/TypeScript syntax with classes, async/await, modules, and full type system.
                </p>
              </article>
              <article className={styles.featureCard}>
                <div className={styles.featureLabel}>Memory safety</div>
                <h3 className={styles.featureTitle}>Ownership model</h3>
                <p className={styles.featureBody}>
                  Rust‑inspired ownership semantics with compile‑time borrow checking for memory safety.
                </p>
              </article>
              <article className={styles.featureCard}>
                <div className={styles.featureLabel}>Architecture</div>
                <h3 className={styles.featureTitle}>Minimal core</h3>
                <p className={styles.featureBody}>
                  Script Core is self‑contained. Extended functionality via optional Rolls ecosystem libraries.
                </p>
              </article>
            </div>
          </div>
        </section>
      </main>
    </Layout>
  );
}
