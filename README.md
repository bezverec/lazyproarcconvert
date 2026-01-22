# LazyProArcConvert ![version](https://img.shields.io/badge/dynamic/toml?url=https://raw.githubusercontent.com/bezverec/lazyproarcconvert/main/Cargo.toml&query=$.package.version&label=version&prefix=v)
TUI nástroj na přípravu TIFF skenů pro [ProArc](https://github.com/proarc/proarc) na pracovních stanicích. S experimentální ALTO prohlížečkou / editorem, podporou **Grok** kodeku a **Tesseract OCR**.

Vznikl z potřeby ulevit serverům a využít výkonné pracovní stanice s OS Windows.


## SW požadavky pro Windows release (vše součástí Windows [releasů](https://github.com/bezverec/lazyproarcconvert/releases))
- **Microsoft Visual C++ 2015-2022 Redistributable Package** (nutno nainstalovat, pokud ještě není)
- **Grok 20.0.4** nebo novější (nemusí se instalovat)
- **Tesseract OCR** (nemusí se instalovat, může se nainstalovat)

## Použití

1. Rozbalte **ZIP** s programem
2. Do adresáře **input** nahrajte 1 nebo více adresářů se skeny
3. Spusťte **lazyproarcconvert.exe**
4. V případě varování Windows povolte spuštění programu
5. Postupujte podle hintů programu
6. Výstupy budou v adresáři **output**
7. Zkopírujte výstupy do importního adresáře své instance ProArcu
8. *Volitelné:* Pro kontrolu a editaci OCR a ALTO můžete spustit **lazyalto.exe** 


## Screenshoty

<img width="1115" height="628" alt="lazyproarcconvert001" src="https://github.com/user-attachments/assets/e998b7a9-9b6c-43b3-ab0b-489ecce7d98b" />


<img width="1269" height="1236" alt="lazyalto001" src="https://github.com/user-attachments/assets/be9861d3-f3cb-4bff-bacd-06aaa77bfad4" />
