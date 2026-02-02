import React from "react";
import { useThemeConfig } from "@docusaurus/theme-common";
import {
  splitNavbarItems,
  useNavbarMobileSidebar,
} from "@docusaurus/theme-common/internal";
import { useLocation } from "@docusaurus/router";
import Link from "@docusaurus/Link";
import NavbarItem, { type Props as NavbarItemConfig } from "@theme/NavbarItem";
import NavbarColorModeToggle from "@theme/Navbar/ColorModeToggle";
import NavbarMobileSidebarToggle from "@theme/Navbar/MobileSidebar/Toggle";
import NavbarLogo from "@theme/Navbar/Logo";
import NavbarSearch from "@theme/Navbar/Search";
import SearchBar from "@theme/SearchBar";
import styles from "./styles.module.css";

function useNavbarItems() {
  return useThemeConfig().navbar.items as NavbarItemConfig[];
}

// Custom tab component with proper active state
function NavbarTab({ to, label, isActive }: { to: string; label: string; isActive: boolean }) {
  return (
    <Link
      to={to}
      className={`navbar__item navbar__link ${styles.navbarTab} ${isActive ? styles.navbarTabActive : ""}`}
    >
      {label}
    </Link>
  );
}

function NavbarContentDesktop() {
  const items = useNavbarItems();
  const location = useLocation();

  // Separate items by their custom classes
  const tabItems = items.filter((item) =>
    item.className?.includes("navbar__item--tab")
  );
  const secondaryItems = items.filter((item) =>
    item.className?.includes("navbar__item--secondary")
  );
  const ctaItems = items.filter((item) =>
    item.className?.includes("navbar__item--cta")
  );
  const searchItem = items.find((item) => item.type === "search");

  // Check if a tab item is active based on current path
  const isTabActive = (item: NavbarItemConfig) => {
    const to = (item as any).to as string | undefined;
    if (!to) return false;
    const basePath = "/" + to.split("/")[1];
    return location.pathname.startsWith(basePath);
  };

  return (
    <div className={styles.navbarDesktop}>
      {/* Row 1: Logo | spacer | Search + CTA + Theme Toggle */}
      <div className={styles.row1}>
        <NavbarLogo />
        <div className={styles.row1Right}>
          <NavbarSearch>
            <SearchBar />
          </NavbarSearch>
          {ctaItems.map((item, i) => (
            <NavbarItem {...item} key={i} />
          ))}
          <NavbarColorModeToggle className={styles.colorModeToggle} />
        </div>
      </div>

      {/* Row 2: Tabs | spacer | Secondary links */}
      <div className={styles.row2}>
        <div className={styles.row2Left}>
          {tabItems.map((item, i) => (
            <NavbarTab
              key={i}
              to={(item as any).to}
              label={(item as any).label}
              isActive={isTabActive(item)}
            />
          ))}
        </div>
        <div className={styles.row2Right}>
          {secondaryItems.map((item, i) => (
            <NavbarItem {...item} key={i} />
          ))}
        </div>
      </div>
    </div>
  );
}

function NavbarContentMobile() {
  const mobileSidebar = useNavbarMobileSidebar();

  return (
    <div className={styles.navbarMobile}>
      {!mobileSidebar.disabled && <NavbarMobileSidebarToggle />}
      <NavbarLogo />
      <div className={styles.mobileRight}>
        <NavbarColorModeToggle className={styles.colorModeToggle} />
      </div>
    </div>
  );
}

export default function NavbarContent(): JSX.Element {
  return (
    <>
      <NavbarContentDesktop />
      <NavbarContentMobile />
    </>
  );
}
