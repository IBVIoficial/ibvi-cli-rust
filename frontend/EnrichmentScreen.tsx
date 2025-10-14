import React, { useState } from "react";

type EnrichResponse = {
  base: {
    id: string;
    name: string;
    cpf?: string;
    birth_date?: string;
    sex?: string;
    mother_name?: string;
    father_name?: string;
    rg?: string;
  };
  emails: { email: string; ranking?: number }[];
  phones: {
    ddd?: string;
    number?: string;
    operator_?: string;
    kind?: string;
    ranking?: number;
  }[];
  addresses: {
    street?: string;
    number?: string;
    neighborhood?: string;
    city?: string;
    uf?: string;
    postal_code?: string;
    complement?: string;
    ranking?: number;
    latitude?: string;
    longitude?: string;
    ddd?: string;
    street_type?: string;
  }[];
};

export default function EnrichmentScreen(): JSX.Element {
  const [cpf, setCpf] = useState("");
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [phone, setPhone] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [data, setData] = useState<EnrichResponse | null>(null);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setData(null);

    const fields: Array<[string, string]> = [];
    if (cpf.trim()) fields.push(["cpf", cpf.trim()]);
    if (name.trim()) fields.push(["name", name.trim()]);
    if (email.trim()) fields.push(["email", email.trim()]);
    if (phone.trim()) fields.push(["phone", phone.trim()]);

    if (fields.length === 0) {
      setError("Enter at least one field");
      return;
    }

    const search_types = fields.map(([k]) => k);
    const searches = fields.map(([, v]) => v);

    setLoading(true);
    try {
      const res = await fetch("/enrich/person", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ search_types, searches }),
      });

      if (res.status === 404) {
        setError("No data found");
      } else if (!res.ok) {
        const msg = await res.text();
        setError(msg || "Server error");
      } else {
        const json = (await res.json()) as EnrichResponse;
        setData(json);
      }
    } catch (err: any) {
      setError(err?.message || "Network error");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div style={{ maxWidth: 860, margin: "0 auto", padding: 24 }}>
      <h1>Diretrix Enrichment</h1>
      <form onSubmit={onSubmit} style={{ display: "grid", gap: 12 }}>
        <input placeholder="CPF" value={cpf} onChange={(e) => setCpf(e.target.value)} />
        <input placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} />
        <input placeholder="Email" value={email} onChange={(e) => setEmail(e.target.value)} />
        <input placeholder="Phone" value={phone} onChange={(e) => setPhone(e.target.value)} />
        <button disabled={loading}>{loading ? "Loading..." : "Submit"}</button>
      </form>

      {error && <p style={{ color: "crimson", marginTop: 16 }}>{error}</p>}

      {data && (
        <div style={{ marginTop: 24 }}>
          <h2>Customer</h2>
          <p>
            <strong>Name:</strong> {data.base.name}
          </p>
          <p>
            <strong>CPF:</strong> {data.base.cpf || "-"}
          </p>
          <p>
            <strong>Birth date:</strong> {data.base.birth_date || "-"}
          </p>
          <p>
            <strong>Sex:</strong> {data.base.sex || "-"}
          </p>

          <h3>Emails</h3>
          {data.emails.length === 0 ? (
            <p>None</p>
          ) : (
            <ul>
              {data.emails.map((email, idx) => (
                <li key={idx}>
                  {email.email}{" "}
                  {email.ranking != null ? <span>(rank {email.ranking})</span> : null}
                </li>
              ))}
            </ul>
          )}

          <h3>Phones</h3>
          {data.phones.length === 0 ? (
            <p>None</p>
          ) : (
            <ul>
              {data.phones.map((phone, idx) => (
                <li key={idx}>
                  {[phone.ddd, phone.number].filter(Boolean).join(" ")}{" "}
                  {phone.operator_ ? `(${phone.operator_})` : ""}{" "}
                  {phone.kind ? `[${phone.kind}]` : ""}{" "}
                  {phone.ranking != null ? `(rank ${phone.ranking})` : ""}
                </li>
              ))}
            </ul>
          )}

          <h3>Addresses</h3>
          {data.addresses.length === 0 ? (
            <p>None</p>
          ) : (
            <ul>
              {data.addresses.map((addr, idx) => (
                <li key={idx}>
                  {[addr.street, addr.number, addr.neighborhood, addr.city, addr.uf, addr.postal_code]
                    .filter(Boolean)
                    .join(", ")}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

