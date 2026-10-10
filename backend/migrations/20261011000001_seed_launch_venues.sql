-- Launch venues in District 2 (Thu Duc City), confirmed by Long 2026-10-10.
-- Coordinates decoded from each venue's Google Maps Plus Code (about ±7 m).
INSERT INTO venues (slug, name, address, location) VALUES
    ('ssa-amitie', 'SSA Sports Center (Amitie Thảo Điền)', '28 Duyên Hải, An Khánh',
        ST_SetSRID(ST_MakePoint(106.738812, 10.806938), 4326)::geography),
    ('an-phu-nguyen-hoang', 'Sân bóng An Phú Quận 2', '93 Nguyễn Hoàng, Bình Trưng',
        ST_SetSRID(ST_MakePoint(106.745812, 10.795813), 4326)::geography),
    ('khu-the-thao-an-phu', 'Khu thể thao An Phú', '250 Mai Chí Thọ, Bình Trưng',
        ST_SetSRID(ST_MakePoint(106.756062, 10.804438), 4326)::geography)
ON CONFLICT (slug) DO NOTHING;
