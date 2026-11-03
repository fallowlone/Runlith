import type { CSSProperties } from "react";
import arrowLeft from "@phosphor-icons/core/assets/regular/arrow-left.svg";
import arrowUp from "@phosphor-icons/core/assets/regular/arrow-up.svg";
import arrowsClockwise from "@phosphor-icons/core/assets/regular/arrows-clockwise.svg";
import at from "@phosphor-icons/core/assets/regular/at.svg";
import bellSimple from "@phosphor-icons/core/assets/regular/bell-simple.svg";
import caretDown from "@phosphor-icons/core/assets/regular/caret-down.svg";
import caretRight from "@phosphor-icons/core/assets/regular/caret-right.svg";
import check from "@phosphor-icons/core/assets/regular/check.svg";
import circleDashed from "@phosphor-icons/core/assets/regular/circle-dashed.svg";
import clockCounterClockwise from "@phosphor-icons/core/assets/regular/clock-counter-clockwise.svg";
import fileCode from "@phosphor-icons/core/assets/regular/file-code.svg";
import gearSix from "@phosphor-icons/core/assets/regular/gear-six.svg";
import gitBranch from "@phosphor-icons/core/assets/regular/git-branch.svg";
import gitCommit from "@phosphor-icons/core/assets/regular/git-commit.svg";
import gitDiff from "@phosphor-icons/core/assets/regular/git-diff.svg";
import graph from "@phosphor-icons/core/assets/regular/graph.svg";
import house from "@phosphor-icons/core/assets/regular/house.svg";
import listChecks from "@phosphor-icons/core/assets/regular/list-checks.svg";
import listDashes from "@phosphor-icons/core/assets/regular/list-dashes.svg";
import magnifyingGlass from "@phosphor-icons/core/assets/regular/magnifying-glass.svg";
import minus from "@phosphor-icons/core/assets/regular/minus.svg";
import monitor from "@phosphor-icons/core/assets/regular/monitor.svg";
import moon from "@phosphor-icons/core/assets/regular/moon.svg";
import plug from "@phosphor-icons/core/assets/regular/plug.svg";
import plugsConnected from "@phosphor-icons/core/assets/regular/plugs-connected.svg";
import plus from "@phosphor-icons/core/assets/regular/plus.svg";
import sealCheck from "@phosphor-icons/core/assets/regular/seal-check.svg";
import shieldCheck from "@phosphor-icons/core/assets/regular/shield-check.svg";
import sidebarSimple from "@phosphor-icons/core/assets/regular/sidebar-simple.svg";
import sparkle from "@phosphor-icons/core/assets/regular/sparkle.svg";
import squaresFour from "@phosphor-icons/core/assets/regular/squares-four.svg";
import sun from "@phosphor-icons/core/assets/regular/sun.svg";
import warningDiamond from "@phosphor-icons/core/assets/regular/warning-diamond.svg";
import slidersHorizontal from "@phosphor-icons/core/assets/regular/sliders-horizontal.svg";
import star from "@phosphor-icons/core/assets/regular/star.svg";
import trash from "@phosphor-icons/core/assets/regular/trash.svg";
import gitPullRequest from "@phosphor-icons/core/assets/regular/git-pull-request.svg";
import x from "@phosphor-icons/core/assets/regular/x.svg";
import xCircle from "@phosphor-icons/core/assets/regular/x-circle.svg";

const ICONS = {
  arrowLeft, arrowUp, arrowsClockwise, at, bellSimple, caretDown, caretRight, check, circleDashed,
  clockCounterClockwise, fileCode, gearSix, gitBranch, gitCommit, gitDiff, gitPullRequest, graph, house, listChecks,
  listDashes, magnifyingGlass, minus, monitor, moon, plug, plugsConnected, plus, sealCheck, shieldCheck,
  sidebarSimple, slidersHorizontal, sparkle, squaresFour, star, sun, trash, warningDiamond, x, xCircle,
} as const;

export type IconName = keyof typeof ICONS;

interface IconProps {
  name: IconName;
  small?: boolean;
  className?: string;
}

/** Phosphor regular glyph painted with currentColor through a CSS mask. Always decorative. */
export function Icon({ name, small = false, className = "" }: IconProps) {
  const style = { "--icon": `url("${ICONS[name]}")` } as CSSProperties;
  return <span className={`i${small ? " i-sm" : ""} ${className}`} style={style} aria-hidden="true" />;
}
