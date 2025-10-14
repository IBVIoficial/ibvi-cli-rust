# Diretrix Scraper with Workbuscas Enrichment

## Quick Start

### 1. Configure Environment Variables

Make sure your `.env` file has the Workbuscas token:

```bash
# Diretrix Scraper Configuration
DIRETRIX_USERNAME=your_username
DIRETRIX_PASSWORD=your_password
DIRETRIX_WEBDRIVER_URL=http://localhost:9515

# Workbuscas Enrichment API
WORKBUSCAS_TOKEN=FXEniLsawoXPlTdYTbdjZAxn
```

### 2. Run the Scraper

```bash
cargo run -- diretrix --street "Street Name" --street-number "123"
```

The scraper will:
1. ✅ Login to Diretrix
2. ✅ Search for properties at the given address
3. ✅ Display results in a formatted table
4. ✅ **Automatically enrich each record** using Workbuscas API
5. ✅ Export enriched data to CSV: `diretrix_streetname_number.csv`

### 3. Output

**Console Output:**
- Property records displayed in a table
- Enrichment status for each owner
- Full enriched profiles (emails, phones, addresses)

**CSV Export:**
- All property data
- `EnrichmentJSON` column with complete enriched data

## Enrichment Features

### What Gets Enriched

For each property owner, the system:
1. **Tries CPF first** (padded to 11 digits)
   - Example: `201120844` → `00201120844`
2. **Falls back to name search** if CPF fails
   - URL-encoded for API calls
3. **Ignores masked CPFs** (with 'X' characters)

### Enriched Data Includes

- **Basic Info:** Name, CPF, birth date, sex, parents' names
- **Contact:** Emails, phone numbers (with operators and types)
- **Addresses:** Complete address history with postal codes

## Example

```bash
# Search for properties on Domingos Leme 440
cargo run -- diretrix --street "Domingos Leme" --street-number "440"

# Output:
# Found 14 record(s) for Domingos Leme 440:
# [Table of properties]
#
# ✅ Using Workbuscas API for enrichment
#
# ✅ Enrichment succeeded for 'EUGENIO ERMIRIO DE MORAES' using CPF 35304791878
# 🔎 Enriched profile:
#   Name: EUGENIO ERMIRIO DE MORAES
#   CPF: 35304791878
#   Birth date: 02/11/1986
#   Emails: eugeniomoraes1102@hotmail.com, ...
#   Phones: ...
#   Addresses: ...
#
# ✅ Results exported to: diretrix_domingos_leme_440.csv
```

## Troubleshooting

### No Enrichment Happening

Check if `WORKBUSCAS_TOKEN` is set:
```bash
grep WORKBUSCAS_TOKEN .env
```

If not present, add it:
```bash
echo "WORKBUSCAS_TOKEN=FXEniLsawoXPlTdYTbdjZAxn" >> .env
```

### ChromeDriver Issues

Make sure ChromeDriver is running:
```bash
# The scraper starts it automatically, but if issues occur:
pkill chromedriver
./start.chromedriver.sh
```

## Architecture

```
Diretrix Scraper → Properties with CPF/Name
                 ↓
        Workbuscas API (GET requests)
                 ↓
        Enriched Data (JSON)
                 ↓
        CSV Export with EnrichmentJSON column
```

**API Endpoints:**
- CPF: `https://completa.workbuscas.com/api?token={TOKEN}&modulo=cpf&consulta={CPF}`
- Name: `https://completa.workbuscas.com/api?token={TOKEN}&modulo=name&consulta={NAME}`

---

**Note:** The local enrichment service (`serve-enrichment`) is optional and not needed when using Workbuscas API.
