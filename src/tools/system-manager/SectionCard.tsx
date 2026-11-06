import type { ReactNode } from "react";

interface SectionCardProps {
  title: string;
  subtitle?: string;
  children: ReactNode;
}

/** One titled card on the System Manager page (Awake, Desktop Icons, …). */
export default function SectionCard({ title, subtitle, children }: SectionCardProps) {
  return (
    <section className="sm-card">
      <h2 className="sm-card-title">{title}</h2>
      {subtitle !== undefined && <p className="sm-card-subtitle">{subtitle}</p>}
      {children}
    </section>
  );
}
