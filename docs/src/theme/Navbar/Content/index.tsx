import React from "react";
import { useThemeConfig } from "@docusaurus/theme-common";
import {
  splitNavbarItems,
  useNavbarMobileSidebar,
} from "@docusaurus/theme-common/internal";
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

function NavbarContentDesktop() {
  const items = useNavbarItems();

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
            <NavbarItem {...item} key={i} />
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
